# Python SDK

## Context

The Python SDK is the primary integration surface for AgentBox. A thin HTTP/WebSocket
client that wraps the daemon API into a clean `Sandbox` class. Also ships pre-built
tool definitions for OpenAI and Anthropic function calling formats.

This task assumes the daemon HTTP API (FAU-72) and WebSocket (FAU-73) are working.
See `docs/architecture.md` for the full SDK design.

## Requirements

- `Sandbox.create()` / `.exec()` / `.upload()` / `.download()` / `.destroy()`
- Context manager support (`with Sandbox.create() as sb:`)
- Streaming exec via WebSocket
- Pre-built tool definitions for OpenAI and Anthropic formats
- `handle_tool_call()` helper for agent loop integration
- Published on PyPI as `agentbox`

## Implementation

### `sdks/python/pyproject.toml`

```toml
[project]
name = "agentbox"
version = "0.1.0"
description = "Self-hosted sandbox infrastructure for AI agents"
readme = "README.md"
license = "Apache-2.0"
requires-python = ">=3.10"
dependencies = [
    "httpx>=0.27",
    "pydantic>=2.0",
    "websockets>=13.0",
]

[project.optional-dependencies]
dev = ["pytest", "pytest-asyncio", "respx>=0.21"]

[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"
```

### `sdks/python/agentbox/__init__.py`

```python
from .sandbox import Sandbox
from .types import ExecResult, FileEntry, SandboxInfo

__all__ = ["Sandbox", "ExecResult", "FileEntry", "SandboxInfo"]
__version__ = "0.1.0"
```

### `sdks/python/agentbox/types.py`

```python
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
```

### `sdks/python/agentbox/client.py`

```python
import os
from typing import Any

import httpx


class AgentBoxClient:
    """HTTP client for the AgentBox daemon API."""

    def __init__(self, url: str | None = None):
        self.base_url = (
            url
            or os.environ.get("AGENTBOX_URL")
            or "http://localhost:8080"
        ).rstrip("/")
        self._client = httpx.Client(base_url=self.base_url, timeout=60.0)

    def post(self, path: str, **kwargs) -> dict:
        resp = self._client.post(path, **kwargs)
        resp.raise_for_status()
        return resp.json()

    def get(self, path: str, **kwargs) -> dict | list:
        resp = self._client.get(path, **kwargs)
        resp.raise_for_status()
        return resp.json()

    def get_bytes(self, path: str, **kwargs) -> bytes:
        resp = self._client.get(path, **kwargs)
        resp.raise_for_status()
        return resp.content

    def delete(self, path: str, **kwargs) -> dict:
        resp = self._client.delete(path, **kwargs)
        resp.raise_for_status()
        return resp.json()

    def ws_url(self, path: str) -> str:
        """Convert HTTP URL to WebSocket URL."""
        return self.base_url.replace("http://", "ws://").replace("https://", "wss://") + path

    def close(self):
        self._client.close()
```

### `sdks/python/agentbox/sandbox.py`

```python
from __future__ import annotations

import asyncio
import json
import base64
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
        try:
            self.destroy()
        except Exception:
            pass

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
```

### `sdks/python/agentbox/tools.py`

```python
from __future__ import annotations

import json
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .sandbox import Sandbox

SANDBOX_TOOLS = [
    {
        "name": "execute_code",
        "description": (
            "Execute a bash command or script in the sandbox. "
            "Use this to run code, install packages, or perform any shell operation."
        ),
        "parameters": {
            "command": {
                "type": "string",
                "description": "The bash command to execute",
            },
        },
        "required": ["command"],
    },
    {
        "name": "write_file",
        "description": "Write content to a file in the sandbox.",
        "parameters": {
            "path": {
                "type": "string",
                "description": "Absolute path in the sandbox",
            },
            "content": {
                "type": "string",
                "description": "File content to write",
            },
        },
        "required": ["path", "content"],
    },
    {
        "name": "read_file",
        "description": "Read the contents of a file in the sandbox.",
        "parameters": {
            "path": {
                "type": "string",
                "description": "Absolute path in the sandbox",
            },
        },
        "required": ["path"],
    },
]


def get_tool_definitions(format: str = "openai") -> list[dict]:
    """Return tool definitions in the specified format."""
    if format == "openai":
        return [
            {
                "type": "function",
                "function": {
                    "name": t["name"],
                    "description": t["description"],
                    "parameters": {
                        "type": "object",
                        "properties": t["parameters"],
                        "required": t["required"],
                    },
                },
            }
            for t in SANDBOX_TOOLS
        ]
    elif format == "anthropic":
        return [
            {
                "name": t["name"],
                "description": t["description"],
                "input_schema": {
                    "type": "object",
                    "properties": t["parameters"],
                    "required": t["required"],
                },
            }
            for t in SANDBOX_TOOLS
        ]
    else:
        return SANDBOX_TOOLS


def handle_tool_call(sandbox: Sandbox, tool_call: dict) -> dict:
    """Execute an LLM tool call against a sandbox.

    Supports both OpenAI and Anthropic tool call formats.
    """
    # Extract name and args from various formats
    name = tool_call.get("name") or tool_call.get("function", {}).get("name")
    args = (
        tool_call.get("input")            # Anthropic
        or tool_call.get("arguments")      # OpenAI (parsed)
        or tool_call.get("function", {}).get("arguments")  # OpenAI (nested)
    )

    if isinstance(args, str):
        try:
            args = json.loads(args)
        except (json.JSONDecodeError, TypeError):
            return {"error": "Invalid JSON in tool call arguments"}

    if not name or not isinstance(args, dict):
        return {"error": "Could not parse tool call"}

    if name == "execute_code":
        if "command" not in args:
            return {"error": "Missing required parameter: command"}
        result = sandbox.exec(args["command"])
        return {
            "stdout": result.stdout,
            "stderr": result.stderr,
            "exit_code": result.exit_code,
        }
    elif name == "write_file":
        missing = [k for k in ("path", "content") if k not in args]
        if missing:
            return {"error": f"Missing required parameters: {', '.join(missing)}"}
        sandbox.upload_content(args["content"].encode(), args["path"])
        return {"status": "written", "path": args["path"]}
    elif name == "read_file":
        if "path" not in args:
            return {"error": "Missing required parameter: path"}
        content = sandbox.download(args["path"])
        return {"content": content.decode("utf-8", errors="replace")}
    else:
        return {"error": f"Unknown tool: {name}"}
```

### `sdks/python/README.md`

Brief README with installation and quickstart:
```markdown
# AgentBox Python SDK

Self-hosted sandbox infrastructure for AI agents.

## Install

pip install agentbox

## Quickstart

from agentbox import Sandbox

with Sandbox.create() as sb:
    result = sb.exec("echo hello world")
    print(result.stdout)  # "hello world\n"
```

## Testing Strategy

Run tests: `cd sdks/python && uv run pytest`

### `sdks/python/tests/test_types.py`:
- `test_exec_result_creation` — create ExecResult, verify fields
- `test_file_entry_creation` — create FileEntry, verify fields

### `sdks/python/tests/test_tools.py`:
- `test_openai_format` — verify get_tool_definitions("openai") returns correct schema
- `test_anthropic_format` — verify get_tool_definitions("anthropic") returns correct schema
- `test_generic_format` — verify get_tool_definitions("generic") returns raw tools
- `test_handle_tool_call_openai` — mock sandbox, call handle_tool_call with OpenAI format
- `test_handle_tool_call_anthropic` — mock sandbox, call handle_tool_call with Anthropic format
- `test_handle_tool_call_unknown` — verify error for unknown tool name

### `sdks/python/tests/test_client.py`:
- Use `respx` to mock HTTP requests
- `test_client_default_url` — verify defaults to localhost:8080
- `test_client_env_url` — verify reads AGENTBOX_URL env var
- `test_client_post` — mock POST, verify request sent correctly
- `test_client_error_handling` — mock 500 response, verify raises

### `sdks/python/tests/test_sandbox.py`:
- Use `respx` to mock all HTTP calls
- `test_sandbox_create` — mock POST /sandboxes, verify Sandbox created
- `test_sandbox_exec` — mock POST /sandboxes/{id}/exec, verify ExecResult
- `test_sandbox_context_manager` — verify destroy called on __exit__
- `test_sandbox_list_files` — mock GET /sandboxes/{id}/files?list=true

## Out of Scope

- Async SDK variant (sync-first, async can wrap later)
- Retry logic / automatic reconnection
- Connection pooling in the HTTP client
- PyPI publishing automation (Task K)
