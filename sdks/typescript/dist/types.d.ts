export interface ExecResult {
    stdout: string;
    stderr: string;
    exit_code: number;
}
export interface FileEntry {
    name: string;
    size: number;
    is_dir: boolean;
}
export interface SandboxInfo {
    id: string;
    status: string;
    config: Record<string, unknown>;
    created_at: string;
}
export interface SandboxConfig {
    memory_mb?: number;
    vcpus?: number;
    network?: boolean;
    timeout?: number;
    disk_size_mb?: number;
}
export interface ExecStreamEvent {
    type: "stdout" | "stderr" | "exit" | "error";
    data?: string;
    code?: number;
    message?: string;
}
export interface PortForwardInfo {
    guest_port: number;
    host_port: number;
    local_address: string;
}
export interface PoolStatus {
    available: number;
    active: number;
    [key: string]: unknown;
}
export interface HealthStatus {
    status: string;
    pool: {
        active: number;
        max_size: number;
    };
}
export type ExecToolResult = {
    stdout: string;
    stderr: string;
    exit_code: number;
};
export type WriteToolResult = {
    status: "written";
    path: string;
};
export type ReadToolResult = {
    content: string;
};
export type ToolError = {
    error: string;
};
export type ToolResult = ExecToolResult | WriteToolResult | ReadToolResult | ToolError;
