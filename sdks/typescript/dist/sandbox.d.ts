import type { ExecResult, ExecStreamEvent, FileEntry, HealthStatus, PoolStatus, PortForwardInfo, SandboxConfig, SandboxInfo, ToolResult } from "./types.js";
/** Interactive exec session with stdin and signal support. */
export declare class ExecSession {
    private readonly ws;
    private readonly messages;
    private resolve;
    private done;
    /** @internal */
    constructor(ws: WebSocket);
    /** Async iterator yielding exec events. */
    events(): AsyncGenerator<ExecStreamEvent>;
    /** Send stdin bytes to the running command. */
    sendStdin(data: Uint8Array): void;
    /** Send a POSIX signal to the running command. */
    sendSignal(signal: number): void;
    /** Close the WebSocket connection. */
    close(): void;
}
/** A sandboxed environment for executing code. */
export declare class Sandbox {
    readonly id: string;
    private readonly client;
    private constructor();
    /** Create a new sandbox. Boots a microVM in <300ms. */
    static create(options?: SandboxConfig & {
        url?: string;
        api_key?: string;
    }): Promise<Sandbox>;
    /** List all active sandboxes. */
    static list(options?: {
        url?: string;
        api_key?: string;
    }): Promise<SandboxInfo[]>;
    /** Get pool status. */
    static poolStatus(options?: {
        url?: string;
        api_key?: string;
    }): Promise<PoolStatus>;
    /** Health check (public, no auth required). */
    static health(options?: {
        url?: string;
    }): Promise<HealthStatus>;
    /** Execute a command and wait for completion. */
    exec(command: string, timeout?: number): Promise<ExecResult>;
    /** Execute with streaming output via WebSocket. Returns an async iterator. */
    execStream(command: string): AsyncGenerator<ExecStreamEvent>;
    /** Start an interactive exec session with stdin and signal support. */
    execInteractive(command: string): Promise<ExecSession>;
    /** Upload content to the sandbox. */
    uploadContent(content: Uint8Array, remotePath: string): Promise<void>;
    /** Download a file from the sandbox. */
    download(remotePath: string): Promise<Uint8Array>;
    /** List files in the sandbox. */
    listFiles(path?: string): Promise<FileEntry[]>;
    /** Delete a file in the sandbox. */
    deleteFile(path: string): Promise<void>;
    /** Create a directory in the sandbox. */
    mkdir(path: string): Promise<void>;
    /** Send a POSIX signal to the sandbox process. */
    sendSignal(signal: number): Promise<void>;
    /** Forward a guest port to a host port. Returns the allocated host address. */
    portForward(guestPort: number): Promise<PortForwardInfo>;
    /** List active port forwards for this sandbox. */
    listPortForwards(): Promise<PortForwardInfo[]>;
    /** Remove a port forward by guest port. */
    removePortForward(guestPort: number): Promise<void>;
    /** Get sandbox info. */
    info(): Promise<SandboxInfo>;
    /** Destroy the sandbox and its VM. */
    destroy(): Promise<void>;
    /** Return tool schemas for LLM function calling. */
    toolDefinitions(format?: "openai" | "anthropic" | "generic"): unknown[];
    /** Execute an LLM tool call against this sandbox. */
    handleToolCall(toolCall: Record<string, unknown>): Promise<ToolResult>;
}
