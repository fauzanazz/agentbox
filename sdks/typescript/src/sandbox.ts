import { AgentBoxClient } from "./client.js";
import { getToolDefinitions, handleToolCall } from "./tools.js";
import type {
  ExecResult,
  ExecStreamEvent,
  FileEntry,
  SandboxConfig,
  SandboxInfo,
} from "./types.js";

/** A sandboxed environment for executing code. */
export class Sandbox {
  readonly id: string;
  private readonly client: AgentBoxClient;

  private constructor(id: string, client: AgentBoxClient) {
    this.id = id;
    this.client = client;
  }

  /** Create a new sandbox. Boots a microVM in <300ms. */
  static async create(options?: SandboxConfig & { url?: string }): Promise<Sandbox> {
    const client = new AgentBoxClient(options?.url);
    const data = (await client.post("/sandboxes", {
      memory_mb: options?.memory_mb ?? 2048,
      vcpus: options?.vcpus ?? 2,
      network: options?.network ?? false,
      timeout: options?.timeout ?? 3600,
    })) as SandboxInfo;
    return new Sandbox(data.id, client);
  }

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

  /** Get sandbox info. */
  async info(): Promise<SandboxInfo> {
    return (await this.client.get(`/sandboxes/${this.id}`)) as SandboxInfo;
  }

  /** Destroy the sandbox and its VM. */
  async destroy(): Promise<void> {
    await this.client.delete(`/sandboxes/${this.id}`);
  }

  /** Return tool schemas for LLM function calling. */
  toolDefinitions(format: "openai" | "anthropic" | "generic" = "openai"): unknown[] {
    return getToolDefinitions(format);
  }

  /** Execute an LLM tool call against this sandbox. */
  async handleToolCall(
    toolCall: Record<string, unknown>,
  ): Promise<Record<string, unknown>> {
    return handleToolCall(this, toolCall);
  }
}
