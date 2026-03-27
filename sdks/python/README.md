# AgentBox Python SDK

Self-hosted sandbox infrastructure for AI agents.

## Install

```bash
pip install agentbox
```

## Quickstart

```python
from agentbox import Sandbox

with Sandbox.create() as sb:
    result = sb.exec("echo hello world")
    print(result.stdout)  # "hello world\n"
```

## Streaming Execution

```python
import asyncio
from agentbox import Sandbox

async def main():
    sb = Sandbox.create()
    async for event in sb.exec_stream("python -c 'print(1+1)'"):
        if event["type"] == "stdout":
            print(event["data"], end="")
        elif event["type"] == "exit":
            print(f"Exit code: {event['code']}")
    sb.destroy()

asyncio.run(main())
```

## LLM Tool Integration

```python
from agentbox import Sandbox

with Sandbox.create() as sb:
    # Get tool definitions for your LLM
    tools = sb.tool_definitions(format="openai")   # or "anthropic"

    # After LLM returns a tool call, execute it
    result = sb.handle_tool_call(tool_call)
```
