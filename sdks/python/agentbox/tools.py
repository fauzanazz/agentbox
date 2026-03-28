from __future__ import annotations

import json
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .async_sandbox import AsyncSandbox
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


def _parse_tool_call(tool_call: dict) -> tuple[str | None, dict | None]:
    """Extract name and args from an OpenAI or Anthropic tool call dict."""
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
            return name, None  # signal invalid JSON

    return name, args if isinstance(args, dict) else None


def _dispatch_sync(sandbox: Sandbox, name: str, args: dict) -> dict:
    """Execute a tool call against a sync sandbox."""
    if name == "execute_code":
        if "command" not in args:
            raise ValueError("Missing required parameter: command")
        result = sandbox.exec(args["command"])
        return {
            "stdout": result.stdout,
            "stderr": result.stderr,
            "exit_code": result.exit_code,
        }
    elif name == "write_file":
        missing = [k for k in ("path", "content") if k not in args]
        if missing:
            raise ValueError(f"Missing required parameters: {', '.join(missing)}")
        content = args["content"]
        if not isinstance(content, str):
            raise ValueError("Parameter 'content' must be a string")
        sandbox.upload_content(content.encode(), args["path"])
        return {"status": "written", "path": args["path"]}
    elif name == "read_file":
        if "path" not in args:
            raise ValueError("Missing required parameter: path")
        content = sandbox.download(args["path"])
        return {"content": content.decode("utf-8", errors="replace")}
    else:
        raise ValueError(f"Unknown tool: {name}")


async def _dispatch_async(sandbox: AsyncSandbox, name: str, args: dict) -> dict:
    """Execute a tool call against an async sandbox."""
    if name == "execute_code":
        if "command" not in args:
            raise ValueError("Missing required parameter: command")
        result = await sandbox.exec(args["command"])
        return {
            "stdout": result.stdout,
            "stderr": result.stderr,
            "exit_code": result.exit_code,
        }
    elif name == "write_file":
        missing = [k for k in ("path", "content") if k not in args]
        if missing:
            raise ValueError(f"Missing required parameters: {', '.join(missing)}")
        content = args["content"]
        if not isinstance(content, str):
            raise ValueError("Parameter 'content' must be a string")
        await sandbox.upload_content(content.encode(), args["path"])
        return {"status": "written", "path": args["path"]}
    elif name == "read_file":
        if "path" not in args:
            raise ValueError("Missing required parameter: path")
        content = await sandbox.download(args["path"])
        return {"content": content.decode("utf-8", errors="replace")}
    else:
        raise ValueError(f"Unknown tool: {name}")


def handle_tool_call(
    sandbox: Sandbox, tool_call: dict, *, raise_on_error: bool = True
) -> dict:
    """Execute an LLM tool call against a sandbox.

    Supports both OpenAI and Anthropic tool call formats.

    Args:
        sandbox: The sandbox to execute against.
        tool_call: The tool call dict from the LLM.
        raise_on_error: If True (default), raise ValueError on bad input
            and propagate sandbox errors. If False, return {"error": ...}
            dicts (legacy behavior).
    """
    name, args = _parse_tool_call(tool_call)

    if args is None and name is not None:
        # Invalid JSON in arguments
        if raise_on_error:
            raise ValueError("Invalid JSON in tool call arguments")
        return {"error": "Invalid JSON in tool call arguments"}

    if not name or args is None:
        if raise_on_error:
            raise ValueError("Could not parse tool call")
        return {"error": "Could not parse tool call"}

    try:
        return _dispatch_sync(sandbox, name, args)
    except ValueError:
        if raise_on_error:
            raise
        import traceback
        return {"error": traceback.format_exc().splitlines()[-1].split(": ", 1)[-1]}


async def handle_tool_call_async(
    sandbox: AsyncSandbox, tool_call: dict, *, raise_on_error: bool = True
) -> dict:
    """Async version of handle_tool_call for AsyncSandbox."""
    name, args = _parse_tool_call(tool_call)

    if args is None and name is not None:
        if raise_on_error:
            raise ValueError("Invalid JSON in tool call arguments")
        return {"error": "Invalid JSON in tool call arguments"}

    if not name or args is None:
        if raise_on_error:
            raise ValueError("Could not parse tool call")
        return {"error": "Could not parse tool call"}

    try:
        return await _dispatch_async(sandbox, name, args)
    except ValueError:
        if raise_on_error:
            raise
        import traceback
        return {"error": traceback.format_exc().splitlines()[-1].split(": ", 1)[-1]}
