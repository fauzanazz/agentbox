from __future__ import annotations

import base64
import json
from typing import AsyncIterator

import websockets

from .client import AgentBoxClient
from .types import ExecResult, FileEntry, PortForwardInfo, SandboxInfo


class Sandbox:
    """A sandboxed environment for executing code."""

    def __init__(self, id: str, client: AgentBoxClient):
        self.id = id
        self._client = client

    @classmethod
    def create(
        cls,
        url: str | None = None,
        memory_mb: int = 2048,
        vcpus: int = 2,
        network: bool = False,
        timeout: int = 3600,
        api_key: str | None = None,
    ) -> Sandbox:
        """Create a new sandbox. Boots a microVM in <300ms."""
        client = AgentBoxClient(url, api_key=api_key)
        data = client.post(
            "/sandboxes",
            json={
                "memory_mb": memory_mb,
                "vcpus": vcpus,
                "network": network,
                "timeout": timeout,
            },
        )
        return cls(id=data["id"], client=client)

    # ── Command execution ───────────────────────────────────────

    def exec(self, command: str, timeout: int = 30) -> ExecResult:
        """Execute a command and wait for completion."""
        data = self._client.post(
            f"/sandboxes/{self.id}/exec",
            json={"command": command, "timeout": timeout},
        )
        return ExecResult(**data)

    async def exec_stream(self, command: str) -> AsyncIterator[dict]:
        """Execute with streaming output via WebSocket."""
        ws_url = self._client.ws_url(f"/sandboxes/{self.id}/ws")
        extra_headers = {}
        if self._client.api_key:
            extra_headers["Authorization"] = f"Bearer {self._client.api_key}"

        async with websockets.connect(ws_url, additional_headers=extra_headers) as ws:
            # Wait for ready
            msg = json.loads(await ws.recv())
            if msg.get("type") != "ready":
                raise RuntimeError(f"Expected ready, got: {msg}")

            # Send exec command
            await ws.send(json.dumps({"type": "exec", "command": command}))

            # Stream responses
            async for raw in ws:
                msg = json.loads(raw)
                msg_type = msg.get("type")

                if msg_type == "stdout":
                    decoded = base64.b64decode(msg["data"]).decode("utf-8", errors="replace")
                    yield {"type": "stdout", "data": decoded}
                elif msg_type == "stderr":
                    decoded = base64.b64decode(msg["data"]).decode("utf-8", errors="replace")
                    yield {"type": "stderr", "data": decoded}
                elif msg_type == "exit":
                    yield {"type": "exit", "code": msg["code"]}
                    break
                elif msg_type == "error":
                    yield {"type": "error", "message": msg["message"]}
                    break

    # ── File operations ─────────────────────────────────────────

    def upload(self, local_path: str, remote_path: str) -> None:
        """Upload a file to the sandbox."""
        with open(local_path, "rb") as f:
            self._client.post(
                f"/sandboxes/{self.id}/files",
                files={"file": f},
                data={"path": remote_path},
            )

    def upload_content(self, content: bytes, remote_path: str) -> None:
        """Upload content directly to the sandbox."""
        self._client.post(
            f"/sandboxes/{self.id}/files",
            files={"file": ("upload", content)},
            data={"path": remote_path},
        )

    def download(self, remote_path: str) -> bytes:
        """Download a file from the sandbox."""
        return self._client.get_bytes(
            f"/sandboxes/{self.id}/files",
            params={"path": remote_path},
        )

    def list_files(self, path: str = "/workspace") -> list[FileEntry]:
        """List files in the sandbox."""
        data = self._client.get(
            f"/sandboxes/{self.id}/files",
            params={"list": "true", "path": path},
        )
        return [FileEntry(**f) for f in data]

    def delete_file(self, path: str) -> None:
        """Delete a file in the sandbox."""
        self._client.delete(
            f"/sandboxes/{self.id}/files",
            params={"path": path},
        )

    def mkdir(self, path: str) -> None:
        """Create a directory in the sandbox."""
        self._client.put(
            f"/sandboxes/{self.id}/files",
            params={"path": path},
        )

    # ── Signals ─────────────────────────────────────────────────

    def send_signal(self, signal: int) -> None:
        """Send a POSIX signal to the sandbox process."""
        self._client.post(
            f"/sandboxes/{self.id}/signal",
            json={"signal": signal},
        )

    # ── Port forwarding ────────────────────────────────────────

    def port_forward(self, guest_port: int) -> PortForwardInfo:
        """Create a port forward from host to guest."""
        data = self._client.post(
            f"/sandboxes/{self.id}/ports",
            json={"guest_port": guest_port},
        )
        return PortForwardInfo(**data)

    def list_port_forwards(self) -> list[PortForwardInfo]:
        """List active port forwards."""
        data = self._client.get(f"/sandboxes/{self.id}/ports")
        return [PortForwardInfo(**p) for p in data["ports"]]

    def remove_port_forward(self, guest_port: int) -> None:
        """Remove a port forward."""
        self._client.delete(f"/sandboxes/{self.id}/ports/{guest_port}")

    # ── Info & lifecycle ────────────────────────────────────────

    def info(self) -> SandboxInfo:
        """Get sandbox info."""
        data = self._client.get(f"/sandboxes/{self.id}")
        return SandboxInfo(**data)

    def destroy(self) -> None:
        """Destroy the sandbox and its VM."""
        self._client.delete(f"/sandboxes/{self.id}")
        self._client.close()

    def __enter__(self) -> Sandbox:
        return self

    def __exit__(self, exc_type, exc_val, exc_tb) -> None:
        try:
            self.destroy()
        except Exception:
            if exc_type is None:
                raise

    # === LLM Tool Definitions ===

    def tool_definitions(self, format: str = "openai") -> list[dict]:
        """Return tool schemas for LLM function calling.

        Args:
            format: "openai", "anthropic", or "generic"
        """
        from .tools import get_tool_definitions
        return get_tool_definitions(format)

    def handle_tool_call(self, tool_call: dict, *, raise_on_error: bool = True) -> dict:
        """Execute an LLM tool call against this sandbox.

        Works with both OpenAI and Anthropic tool call formats.

        Args:
            tool_call: The tool call dict from the LLM.
            raise_on_error: If True (default), raise exceptions on errors.
                If False, return {"error": ...} dicts (legacy behavior).
        """
        from .tools import handle_tool_call
        return handle_tool_call(self, tool_call, raise_on_error=raise_on_error)
