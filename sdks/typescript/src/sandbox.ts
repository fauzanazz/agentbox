import { AgentBoxClient } from "./client.js";
import { getToolDefinitions, handleToolCall } from "./tools.js";
import type {
  ExecResult,
  ExecStreamEvent,
  FileEntry,
  HealthStatus,
  PoolStatus,
  PortForwardInfo,
  SandboxConfig,
  SandboxInfo,
  ToolResult,
} from "./types.js";

/** Interactive exec session with stdin and signal support. */
export class ExecSession {
  private readonly ws: WebSocket;
  private readonly messages: ExecStreamEvent[] = [];
  private resolve: (() => void) | null = null;
  private done = false;

  /** @internal */
  constructor(ws: WebSocket) {
    this.ws = ws;

    ws.onmessage = (event) => {
      const msg = JSON.parse(event.data as string);
      const type = msg.type as string;

      if (type === "stdout" || type === "stderr") {
        this.messages.push({ type, data: atob(msg.data) });
      } else if (type === "exit") {
        this.messages.push({ type: "exit", code: msg.code });
        this.done = true;
      } else if (type === "error") {
        this.messages.push({ type: "error", message: msg.message });
        this.done = true;
      }

      if (this.resolve) {
        this.resolve();
        this.resolve = null;
      }
    };

    ws.onerror = () => {
      this.messages.push({
        type: "error",
        message: "WebSocket error occurred",
      });
      this.done = true;
      if (this.resolve) {
        this.resolve();
        this.resolve = null;
      }
    };

    ws.onclose = () => {
      this.done = true;
      if (this.resolve) {
        this.resolve();
        this.resolve = null;
      }
    };
  }

  /** Async iterator yielding exec events. */
  async *events(): AsyncGenerator<ExecStreamEvent> {
    try {
      while (true) {
        if (this.messages.length > 0) {
          const msg = this.messages.shift()!;
          yield msg;
          if (msg.type === "exit" || msg.type === "error") break;
        } else if (this.done) {
          break;
        } else {
          await new Promise<void>((r) => {
            this.resolve = r;
          });
        }
      }
    } finally {
      this.ws.close();
    }
  }

  /** Send stdin bytes to the running command. */
  sendStdin(data: Uint8Array): void {
    const encoded = btoa(
      Array.from(data)
        .map((b) => String.fromCharCode(b))
        .join(""),
    );
    this.ws.send(JSON.stringify({ type: "stdin", data: encoded }));
  }

  /** Send a POSIX signal to the running command. */
  sendSignal(signal: number): void {
    this.ws.send(JSON.stringify({ type: "signal", signal }));
  }

  /** Close the WebSocket connection. */
  close(): void {
    this.ws.close();
  }
}

/** A sandboxed environment for executing code. */
export class Sandbox {
  readonly id: string;
  private readonly client: AgentBoxClient;

  private constructor(id: string, client: AgentBoxClient) {
    this.id = id;
    this.client = client;
  }

  /** Create a new sandbox. Boots a microVM in <300ms. */
  static async create(
    options?: SandboxConfig & { url?: string; api_key?: string },
  ): Promise<Sandbox> {
    const client = new AgentBoxClient(options?.url, options?.api_key);
    const body: Record<string, unknown> = {
      memory_mb: options?.memory_mb ?? 2048,
      vcpus: options?.vcpus ?? 2,
      network: options?.network ?? false,
      timeout: options?.timeout ?? 3600,
    };
    if (options?.disk_size_mb) body.disk_size_mb = options.disk_size_mb;
    const data = (await client.post("/sandboxes", body)) as SandboxInfo;
    return new Sandbox(data.id, client);
  }

  // ── Static API methods ──────────────────────────────────────

  /** List all active sandboxes. */
  static async list(options?: {
    url?: string;
    api_key?: string;
  }): Promise<SandboxInfo[]> {
    const client = new AgentBoxClient(options?.url, options?.api_key);
    return (await client.listSandboxes()) as SandboxInfo[];
  }

  /** Get pool status. */
  static async poolStatus(options?: {
    url?: string;
    api_key?: string;
  }): Promise<PoolStatus> {
    const client = new AgentBoxClient(options?.url, options?.api_key);
    return (await client.poolStatus()) as PoolStatus;
  }

  /** Health check (public, no auth required). */
  static async health(options?: { url?: string }): Promise<HealthStatus> {
    const client = new AgentBoxClient(options?.url);
    return (await client.health()) as HealthStatus;
  }

  // ── Command execution ───────────────────────────────────────

  /** Execute a command and wait for completion. */
  async exec(command: string, timeout = 30): Promise<ExecResult> {
    return (await this.client.post(`/sandboxes/${this.id}/exec`, {
      command,
      timeout,
    })) as ExecResult;
  }

  /** Execute with streaming output via WebSocket. Returns an async iterator. */
  async *execStream(command: string): AsyncGenerator<ExecStreamEvent> {
    const wsUrl = this.client.wsUrl(`/sandboxes/${this.id}/ws`);
    const ws = new WebSocket(wsUrl);

    const messages: ExecStreamEvent[] = [];
    let resolve: (() => void) | null = null;
    let done = false;

    ws.onmessage = (event) => {
      const msg = JSON.parse(event.data as string);
      const type = msg.type as string;

      if (type === "ready") {
        ws.send(JSON.stringify({ type: "exec", command }));
        return;
      }

      if (type === "stdout" || type === "stderr") {
        const decoded = atob(msg.data);
        messages.push({ type, data: decoded });
      } else if (type === "exit") {
        messages.push({ type: "exit", code: msg.code });
        done = true;
      } else if (type === "error") {
        messages.push({ type: "error", message: msg.message });
        done = true;
      }

      if (resolve) {
        resolve();
        resolve = null;
      }
    };

    ws.onerror = () => {
      messages.push({
        type: "error",
        message: "WebSocket error occurred",
      });
      done = true;
      if (resolve) {
        resolve();
        resolve = null;
      }
    };

    ws.onclose = () => {
      done = true;
      if (resolve) {
        resolve();
        resolve = null;
      }
    };

    // Wait for WebSocket to open
    await new Promise<void>((res, rej) => {
      ws.onopen = () => res();
      ws.onerror = () => rej(new Error("WebSocket connection failed"));
    });

    // Restore the streaming error handler (onopen promise overwrites it)
    ws.onerror = () => {
      messages.push({
        type: "error",
        message: "WebSocket error occurred",
      });
      done = true;
      if (resolve) {
        resolve();
        resolve = null;
      }
    };

    try {
      while (true) {
        if (messages.length > 0) {
          const msg = messages.shift()!;
          yield msg;
          if (msg.type === "exit" || msg.type === "error") break;
        } else if (done) {
          break;
        } else {
          await new Promise<void>((r) => {
            resolve = r;
          });
        }
      }
    } finally {
      ws.close();
    }
  }

  /** Start an interactive exec session with stdin and signal support. */
  async execInteractive(command: string): Promise<ExecSession> {
    const wsUrl = this.client.wsUrl(`/sandboxes/${this.id}/ws`);
    const ws = new WebSocket(wsUrl);

    // Wait for open
    await new Promise<void>((res, rej) => {
      ws.onopen = () => res();
      ws.onerror = () => rej(new Error("WebSocket connection failed"));
    });

    // Wait for ready
    await new Promise<void>((res, rej) => {
      ws.onmessage = (event) => {
        const msg = JSON.parse(event.data as string);
        if (msg.type === "ready") {
          res();
        } else {
          rej(new Error(`Expected ready, got: ${JSON.stringify(msg)}`));
        }
      };
      ws.onerror = () =>
        rej(new Error("WebSocket error while waiting for ready"));
    });

    // Send exec command
    ws.send(JSON.stringify({ type: "exec", command }));

    return new ExecSession(ws);
  }

  // ── File operations ─────────────────────────────────────────

  /** Upload content to the sandbox. */
  async uploadContent(content: Uint8Array, remotePath: string): Promise<void> {
    const form = new FormData();
    form.append("path", remotePath);
    form.append("file", new Blob([content]), "upload");
    await this.client.postMultipart(`/sandboxes/${this.id}/files`, form);
  }

  /** Download a file from the sandbox. */
  async download(remotePath: string): Promise<Uint8Array> {
    const buf = await this.client.getBytes(`/sandboxes/${this.id}/files`, {
      path: remotePath,
    });
    return new Uint8Array(buf);
  }

  /** List files in the sandbox. */
  async listFiles(path = "/workspace"): Promise<FileEntry[]> {
    return (await this.client.get(`/sandboxes/${this.id}/files`, {
      list: "true",
      path,
    })) as FileEntry[];
  }

  /** Delete a file in the sandbox. */
  async deleteFile(path: string): Promise<void> {
    await this.client.delete(`/sandboxes/${this.id}/files`, { path });
  }

  /** Create a directory in the sandbox. */
  async mkdir(path: string): Promise<void> {
    await this.client.put(`/sandboxes/${this.id}/files`, { path });
  }

  // ── Signals ─────────────────────────────────────────────────

  /** Send a POSIX signal to the sandbox process. */
  async sendSignal(signal: number): Promise<void> {
    await this.client.post(`/sandboxes/${this.id}/signal`, { signal });
  }

  // ── Port forwarding ────────────────────────────────────────

  /** Forward a guest port to a host port. Returns the allocated host address. */
  async portForward(guestPort: number): Promise<PortForwardInfo> {
    return (await this.client.post(`/sandboxes/${this.id}/ports`, {
      guest_port: guestPort,
    })) as PortForwardInfo;
  }

  /** List active port forwards for this sandbox. */
  async listPortForwards(): Promise<PortForwardInfo[]> {
    const data = (await this.client.get(
      `/sandboxes/${this.id}/ports`,
    )) as { ports: PortForwardInfo[] };
    return data.ports;
  }

  /** Remove a port forward by guest port. */
  async removePortForward(guestPort: number): Promise<void> {
    await this.client.delete(`/sandboxes/${this.id}/ports/${guestPort}`);
  }

  // ── Info & lifecycle ────────────────────────────────────────

  /** Get sandbox info. */
  async info(): Promise<SandboxInfo> {
    return (await this.client.get(`/sandboxes/${this.id}`)) as SandboxInfo;
  }

  /** Destroy the sandbox and its VM. */
  async destroy(): Promise<void> {
    await this.client.delete(`/sandboxes/${this.id}`);
  }

  // ── LLM Tools ───────────────────────────────────────────────

  /** Return tool schemas for LLM function calling. */
  toolDefinitions(
    format: "openai" | "anthropic" | "generic" = "openai",
  ): unknown[] {
    return getToolDefinitions(format);
  }

  /** Execute an LLM tool call against this sandbox. */
  async handleToolCall(
    toolCall: Record<string, unknown>,
  ): Promise<ToolResult> {
    return handleToolCall(this, toolCall);
  }
}
