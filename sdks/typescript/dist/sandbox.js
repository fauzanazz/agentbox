import { AgentBoxClient } from "./client.js";
import { getToolDefinitions, handleToolCall } from "./tools.js";
/** Interactive exec session with stdin and signal support. */
export class ExecSession {
    ws;
    messages = [];
    resolve = null;
    done = false;
    /** @internal */
    constructor(ws) {
        this.ws = ws;
        ws.onmessage = (event) => {
            const msg = JSON.parse(event.data);
            const type = msg.type;
            if (type === "stdout" || type === "stderr") {
                this.messages.push({ type, data: atob(msg.data) });
            }
            else if (type === "exit") {
                this.messages.push({ type: "exit", code: msg.code });
                this.done = true;
            }
            else if (type === "error") {
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
    async *events() {
        try {
            while (true) {
                if (this.messages.length > 0) {
                    const msg = this.messages.shift();
                    yield msg;
                    if (msg.type === "exit" || msg.type === "error")
                        break;
                }
                else if (this.done) {
                    break;
                }
                else {
                    await new Promise((r) => {
                        this.resolve = r;
                    });
                }
            }
        }
        finally {
            this.ws.close();
        }
    }
    /** Send stdin bytes to the running command. */
    sendStdin(data) {
        const encoded = btoa(Array.from(data)
            .map((b) => String.fromCharCode(b))
            .join(""));
        this.ws.send(JSON.stringify({ type: "stdin", data: encoded }));
    }
    /** Send a POSIX signal to the running command. */
    sendSignal(signal) {
        this.ws.send(JSON.stringify({ type: "signal", signal }));
    }
    /** Close the WebSocket connection. */
    close() {
        this.ws.close();
    }
}
/** A sandboxed environment for executing code. */
export class Sandbox {
    id;
    client;
    constructor(id, client) {
        this.id = id;
        this.client = client;
    }
    /** Create a new sandbox. Boots a microVM in <300ms. */
    static async create(options) {
        const client = new AgentBoxClient(options?.url, options?.api_key);
        const body = {
            memory_mb: options?.memory_mb ?? 2048,
            vcpus: options?.vcpus ?? 2,
            network: options?.network ?? false,
            timeout: options?.timeout ?? 3600,
        };
        if (options?.disk_size_mb)
            body.disk_size_mb = options.disk_size_mb;
        const data = (await client.post("/sandboxes", body));
        return new Sandbox(data.id, client);
    }
    // ── Static API methods ──────────────────────────────────────
    /** List all active sandboxes. */
    static async list(options) {
        const client = new AgentBoxClient(options?.url, options?.api_key);
        return (await client.listSandboxes());
    }
    /** Get pool status. */
    static async poolStatus(options) {
        const client = new AgentBoxClient(options?.url, options?.api_key);
        return (await client.poolStatus());
    }
    /** Health check (public, no auth required). */
    static async health(options) {
        const client = new AgentBoxClient(options?.url);
        return (await client.health());
    }
    // ── Command execution ───────────────────────────────────────
    /** Execute a command and wait for completion. */
    async exec(command, timeout = 30) {
        return (await this.client.post(`/sandboxes/${this.id}/exec`, {
            command,
            timeout,
        }));
    }
    /** Execute with streaming output via WebSocket. Returns an async iterator. */
    async *execStream(command) {
        const wsUrl = this.client.wsUrl(`/sandboxes/${this.id}/ws`);
        const ws = new WebSocket(wsUrl);
        const messages = [];
        let resolve = null;
        let done = false;
        ws.onmessage = (event) => {
            const msg = JSON.parse(event.data);
            const type = msg.type;
            if (type === "ready") {
                ws.send(JSON.stringify({ type: "exec", command }));
                return;
            }
            if (type === "stdout" || type === "stderr") {
                const decoded = atob(msg.data);
                messages.push({ type, data: decoded });
            }
            else if (type === "exit") {
                messages.push({ type: "exit", code: msg.code });
                done = true;
            }
            else if (type === "error") {
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
        await new Promise((res, rej) => {
            ws.onopen = () => res();
            ws.onerror = () => rej(new Error("WebSocket connection failed"));
        });
        try {
            while (true) {
                if (messages.length > 0) {
                    const msg = messages.shift();
                    yield msg;
                    if (msg.type === "exit" || msg.type === "error")
                        break;
                }
                else if (done) {
                    break;
                }
                else {
                    await new Promise((r) => {
                        resolve = r;
                    });
                }
            }
        }
        finally {
            ws.close();
        }
    }
    /** Start an interactive exec session with stdin and signal support. */
    async execInteractive(command) {
        const wsUrl = this.client.wsUrl(`/sandboxes/${this.id}/ws`);
        const ws = new WebSocket(wsUrl);
        // Wait for open
        await new Promise((res, rej) => {
            ws.onopen = () => res();
            ws.onerror = () => rej(new Error("WebSocket connection failed"));
        });
        // Wait for ready
        await new Promise((res, rej) => {
            ws.onmessage = (event) => {
                const msg = JSON.parse(event.data);
                if (msg.type === "ready") {
                    res();
                }
                else {
                    rej(new Error(`Expected ready, got: ${JSON.stringify(msg)}`));
                }
            };
        });
        // Send exec command
        ws.send(JSON.stringify({ type: "exec", command }));
        return new ExecSession(ws);
    }
    // ── File operations ─────────────────────────────────────────
    /** Upload content to the sandbox. */
    async uploadContent(content, remotePath) {
        const form = new FormData();
        form.append("path", remotePath);
        form.append("file", new Blob([content]), "upload");
        await this.client.postMultipart(`/sandboxes/${this.id}/files`, form);
    }
    /** Download a file from the sandbox. */
    async download(remotePath) {
        const buf = await this.client.getBytes(`/sandboxes/${this.id}/files`, {
            path: remotePath,
        });
        return new Uint8Array(buf);
    }
    /** List files in the sandbox. */
    async listFiles(path = "/workspace") {
        return (await this.client.get(`/sandboxes/${this.id}/files`, {
            list: "true",
            path,
        }));
    }
    /** Delete a file in the sandbox. */
    async deleteFile(path) {
        await this.client.delete(`/sandboxes/${this.id}/files?path=${encodeURIComponent(path)}`);
    }
    /** Create a directory in the sandbox. */
    async mkdir(path) {
        await this.client.put(`/sandboxes/${this.id}/files`, { path });
    }
    // ── Signals ─────────────────────────────────────────────────
    /** Send a POSIX signal to the sandbox process. */
    async sendSignal(signal) {
        await this.client.post(`/sandboxes/${this.id}/signal`, { signal });
    }
    // ── Port forwarding ────────────────────────────────────────
    /** Forward a guest port to a host port. Returns the allocated host address. */
    async portForward(guestPort) {
        return (await this.client.post(`/sandboxes/${this.id}/ports`, {
            guest_port: guestPort,
        }));
    }
    /** List active port forwards for this sandbox. */
    async listPortForwards() {
        const data = (await this.client.get(`/sandboxes/${this.id}/ports`));
        return data.ports;
    }
    /** Remove a port forward by guest port. */
    async removePortForward(guestPort) {
        await this.client.delete(`/sandboxes/${this.id}/ports/${guestPort}`);
    }
    // ── Info & lifecycle ────────────────────────────────────────
    /** Get sandbox info. */
    async info() {
        return (await this.client.get(`/sandboxes/${this.id}`));
    }
    /** Destroy the sandbox and its VM. */
    async destroy() {
        await this.client.delete(`/sandboxes/${this.id}`);
    }
    // ── LLM Tools ───────────────────────────────────────────────
    /** Return tool schemas for LLM function calling. */
    toolDefinitions(format = "openai") {
        return getToolDefinitions(format);
    }
    /** Execute an LLM tool call against this sandbox. */
    async handleToolCall(toolCall) {
        return handleToolCall(this, toolCall);
    }
}
