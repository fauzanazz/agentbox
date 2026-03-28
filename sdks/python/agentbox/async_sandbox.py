from __future__ import annotations

import base64
import json
from typing import AsyncIterator

import websockets

from .async_client import AsyncAgentBoxClient
from .exec_session import ExecSession
from .types import ExecResult, FileEntry, PortForwardInfo, SandboxInfo


class AsyncSandbox:
    """Async sandboxed environment for executing code."""

    def __init__(self, id: str, client: AsyncAgentBoxClient):
        self.id = id
        self._client = client

    @classmethod
    async def create(
        cls,
        url: str | None = None,
        memory_mb: int = 2048,
        vcpus: int = 2,
        network: bool = False,
        timeout: int = 3600,
        api_key: str | None = None,
    ) -> AsyncSandbox:
        """Create a new sandbox. Boots a microVM in <300ms."""
        client = AsyncAgentBoxClient(url, api_key=api_key)
        data = await client.post(
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

    async def exec(self, command: str, timeout: int = 30) -> ExecResult:
        """Execute a command and wait for completion."""
        data = await self._client.post(
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
            msg = json.loads(await ws.recv())
            if msg.get("type") != "ready":
                raise RuntimeError(f"Expected ready, got: {msg}")

            await ws.send(json.dumps({"type": "exec", "command": command}))

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

    async def exec_interactive(self, command: str) -> ExecSession:
        """Start an interactive exec session with stdin and signal support."""
        ws_url = self._client.ws_url(f"/sandboxes/{self.id}/ws")
        extra_headers = {}
        if self._client.api_key:
            extra_headers["Authorization"] = f"Bearer {self._client.api_key}"

        ws = await websockets.connect(ws_url, additional_headers=extra_headers)

        msg = json.loads(await ws.recv())
        if msg.get("type") != "ready":
            await ws.close()
            raise RuntimeError(f"Expected ready, got: {msg}")

        await ws.send(json.dumps({"type": "exec", "command": command}))
        return ExecSession(ws)

    # ── File operations ─────────────────────────────────────────

    async def upload_content(self, content: bytes, remote_path: str) -> None:
        """Upload content directly to the sandbox."""
        await self._client.post(
            f"/sandboxes/{self.id}/files",
            files={"file": ("upload", content)},
            data={"path": remote_path},
        )

    async def download(self, remote_path: str) -> bytes:
        """Download a file from the sandbox."""
        return await self._client.get_bytes(
            f"/sandboxes/{self.id}/files",
            params={"path": remote_path},
        )

    async def list_files(self, path: str = "/workspace") -> list[FileEntry]:
        """List files in the sandbox."""
        data = await self._client.get(
            f"/sandboxes/{self.id}/files",
            params={"list": "true", "path": path},
        )
        return [FileEntry(**f) for f in data]

    async def delete_file(self, path: str) -> None:
        """Delete a file in the sandbox."""
        await self._client.delete(
            f"/sandboxes/{self.id}/files",
            params={"path": path},
        )

    async def mkdir(self, path: str) -> None:
        """Create a directory in the sandbox."""
        await self._client.put(
            f"/sandboxes/{self.id}/files",
            params={"path": path},
        )

    # ── Signals ─────────────────────────────────────────────────

    async def send_signal(self, signal: int) -> None:
        """Send a POSIX signal to the sandbox process."""
        await self._client.post(
            f"/sandboxes/{self.id}/signal",
            json={"signal": signal},
        )

    # ── Port forwarding ────────────────────────────────────────

    async def port_forward(self, guest_port: int) -> PortForwardInfo:
        """Create a port forward from host to guest."""
        data = await self._client.post(
            f"/sandboxes/{self.id}/ports",
            json={"guest_port": guest_port},
        )
        return PortForwardInfo(**data)

    async def list_port_forwards(self) -> list[PortForwardInfo]:
        """List active port forwards."""
        data = await self._client.get(f"/sandboxes/{self.id}/ports")
        return [PortForwardInfo(**p) for p in data["ports"]]

    async def remove_port_forward(self, guest_port: int) -> None:
        """Remove a port forward."""
        await self._client.delete(f"/sandboxes/{self.id}/ports/{guest_port}")

    # ── Info & lifecycle ────────────────────────────────────────

    async def info(self) -> SandboxInfo:
        """Get sandbox info."""
        data = await self._client.get(f"/sandboxes/{self.id}")
        return SandboxInfo(**data)

    async def destroy(self) -> None:
        """Destroy the sandbox and its VM."""
        await self._client.delete(f"/sandboxes/{self.id}")
        await self._client.close()

    async def __aenter__(self) -> AsyncSandbox:
        return self

    async def __aexit__(self, exc_type, exc_val, exc_tb) -> None:
        try:
            await self.destroy()
        except Exception:
            if exc_type is None:
                raise

    # === LLM Tool Definitions ===

    def tool_definitions(self, format: str = "openai") -> list[dict]:
        """Return tool schemas for LLM function calling."""
        from .tools import get_tool_definitions
        return get_tool_definitions(format)

    async def handle_tool_call(self, tool_call: dict, *, raise_on_error: bool = True) -> dict:
        """Execute an LLM tool call against this sandbox (async version)."""
        from .tools import handle_tool_call_async
        return await handle_tool_call_async(self, tool_call, raise_on_error=raise_on_error)
