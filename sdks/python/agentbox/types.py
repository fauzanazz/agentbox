from pydantic import BaseModel


class ExecResult(BaseModel):
    stdout: str
    stderr: str
    exit_code: int


class FileEntry(BaseModel):
    name: str
    size: int
    is_dir: bool


class SandboxInfo(BaseModel):
    id: str
    status: str
    config: dict
    created_at: str


class PortForwardInfo(BaseModel):
    guest_port: int
    host_port: int
    local_address: str
