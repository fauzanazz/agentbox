from __future__ import annotations

import base64
import json
from typing import AsyncIterator

import websockets

from .client import AgentBoxClient
from .types import ExecResult, FileEntry, SandboxInfo


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
    ) -> Sandbox:
        """Create a new sandbox. Boots a microVM in <300ms."""
        client = AgentBoxClient(url)
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
        async with websockets.connect(ws_url) as ws:
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

    def __exit__(self, *args) -> None:
        self.destroy()

    # === LLM Tool Definitions ===

    def tool_definitions(self, format: str = "openai") -> list[dict]:
        """Return tool schemas for LLM function calling.

        Args:
            format: "openai", "anthropic", or "generic"
        """
        from .tools import get_tool_definitions
        return get_tool_definitions(format)

    def handle_tool_call(self, tool_call: dict) -> dict:
        """Execute an LLM tool call against this sandbox.

        Works with both OpenAI and Anthropic tool call formats.
        """
        from .tools import handle_tool_call
        return handle_tool_call(self, tool_call)
