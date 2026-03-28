"""AgentBox error hierarchy.

Maps daemon HTTP error responses to typed Python exceptions.
"""

from __future__ import annotations


class AgentBoxError(Exception):
    """Base exception for all AgentBox errors."""

    def __init__(
        self,
        message: str,
        status_code: int | None = None,
        response_body: str | None = None,
    ):
        super().__init__(message)
        self.status_code = status_code
        self.response_body = response_body


class AuthenticationError(AgentBoxError):
    """401 — Invalid or missing API key."""


class SandboxNotFoundError(AgentBoxError):
    """404 — Sandbox not found."""


class PathTraversalError(AgentBoxError):
    """400 — Path escapes /workspace."""


class SandboxInUseError(AgentBoxError):
    """400 — Sandbox is held by another request."""


class PoolExhaustedError(AgentBoxError):
    """503 — No sandboxes available in the pool."""


class AgentBoxAPIError(AgentBoxError):
    """Catch-all for other HTTP errors from the daemon."""


def _raise_for_status(response) -> None:
    """Raise a typed AgentBoxError if the response is not 2xx.

    Args:
        response: An httpx.Response object.
    """
    if response.is_success:
        return

    status = response.status_code
    try:
        body = response.text
    except Exception:
        body = ""

    # Try to extract the error message from JSON
    message = body
    try:
        data = response.json()
        if isinstance(data, dict) and "error" in data:
            message = data["error"]
    except Exception:
        pass

    if status == 401:
        raise AuthenticationError(message, status, body)
    elif status == 404:
        raise SandboxNotFoundError(message, status, body)
    elif status == 503:
        raise PoolExhaustedError(message, status, body)
    elif status == 400:
        lower = message.lower()
        if "path" in lower and ("traversal" in lower or "workspace" in lower):
            raise PathTraversalError(message, status, body)
        elif "in use" in lower:
            raise SandboxInUseError(message, status, body)
        else:
            raise AgentBoxAPIError(message, status, body)
    else:
        raise AgentBoxAPIError(message, status, body)
