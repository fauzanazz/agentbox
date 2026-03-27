import { AgentBoxClient } from "./client.js";
import { getToolDefinitions, handleToolCall } from "./tools.js";
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
        const client = new AgentBoxClient(options?.url);
        const data = (await client.post("/sandboxes", {
            memory_mb: options?.memory_mb ?? 2048,
            vcpus: options?.vcpus ?? 2,
            network: options?.network ?? false,
            timeout: options?.timeout ?? 3600,
        }));
        return new Sandbox(data.id, client);
    }
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
    /** Get sandbox info. */
    async info() {
        return (await this.client.get(`/sandboxes/${this.id}`));
    }
    /** Destroy the sandbox and its VM. */
    async destroy() {
        await this.client.delete(`/sandboxes/${this.id}`);
    }
    /** Return tool schemas for LLM function calling. */
    toolDefinitions(format = "openai") {
        return getToolDefinitions(format);
    }
    /** Execute an LLM tool call against this sandbox. */
    async handleToolCall(toolCall) {
        return handleToolCall(this, toolCall);
    }
}
