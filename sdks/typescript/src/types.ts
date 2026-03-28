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
