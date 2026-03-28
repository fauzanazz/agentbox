"""Interactive WebSocket exec session with stdin and signal support."""

from __future__ import annotations

import base64
import json
from typing import AsyncIterator

import websockets


class ExecSession:
    """Interactive command session over WebSocket.

    Supports streaming stdout/stderr, sending stdin, and sending signals.

    Usage::

        async with sandbox.exec_interactive("python3") as session:
            async for event in session.events():
                if event["type"] == "stdout":
                    print(event["data"], end="")
                elif event["type"] == "exit":
                    break
            await session.send_stdin(b"print('hello')\\n")
    """

    def __init__(self, ws: websockets.ClientConnection):
        self._ws = ws

    async def events(self) -> AsyncIterator[dict]:
        """Yield decoded events from the server.

        Event types: stdout, stderr, exit, error.
        stdout/stderr events have a 'data' field (decoded string).
        exit events have a 'code' field (int).
        error events have a 'message' field (str).
        """
        async for raw in self._ws:
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

    async def send_stdin(self, data: bytes) -> None:
        """Send stdin bytes to the running command."""
        encoded = base64.b64encode(data).decode("ascii")
        await self._ws.send(json.dumps({"type": "stdin", "data": encoded}))

    async def send_signal(self, signal: int) -> None:
        """Send a POSIX signal to the running command."""
        await self._ws.send(json.dumps({"type": "signal", "signal": signal}))

    async def close(self) -> None:
        """Close the WebSocket connection."""
        await self._ws.close()

    async def __aenter__(self) -> ExecSession:
        return self

    async def __aexit__(self, exc_type, exc_val, exc_tb) -> None:
        await self.close()
