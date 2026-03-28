import type { ExecResult, ExecStreamEvent, FileEntry, PortForwardInfo, SandboxConfig, SandboxInfo } from "./types.js";
/** A sandboxed environment for executing code. */
export declare class Sandbox {
    readonly id: string;
    private readonly client;
    private constructor();
    /** Create a new sandbox. Boots a microVM in <300ms. */
    static create(options?: SandboxConfig & {
        url?: string;
    }): Promise<Sandbox>;
    /** Execute a command and wait for completion. */
    exec(command: string, timeout?: number): Promise<ExecResult>;
    /** Execute with streaming output via WebSocket. Returns an async iterator. */
    execStream(command: string): AsyncGenerator<ExecStreamEvent>;
    /** Upload content to the sandbox. */
    uploadContent(content: Uint8Array, remotePath: string): Promise<void>;
    /** Download a file from the sandbox. */
    download(remotePath: string): Promise<Uint8Array>;
    /** List files in the sandbox. */
    listFiles(path?: string): Promise<FileEntry[]>;
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
    handleToolCall(toolCall: Record<string, unknown>): Promise<Record<string, unknown>>;
}
