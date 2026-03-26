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
