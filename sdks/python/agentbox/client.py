import os

import httpx

from .errors import _raise_for_status


class AgentBoxClient:
    """HTTP client for the AgentBox daemon API."""

    def __init__(self, url: str | None = None, api_key: str | None = None):
        self.base_url = (
            url
            or os.environ.get("AGENTBOX_URL")
            or "http://localhost:8080"
        ).rstrip("/")

        self.api_key = api_key or os.environ.get("AGENTBOX_API_KEY")

        headers = {}
        if self.api_key:
            headers["Authorization"] = f"Bearer {self.api_key}"

        self._client = httpx.Client(
            base_url=self.base_url, timeout=60.0, headers=headers
        )

    def post(self, path: str, **kwargs) -> dict:
        resp = self._client.post(path, **kwargs)
        _raise_for_status(resp)
        return resp.json()

    def get(self, path: str, **kwargs) -> dict | list:
        resp = self._client.get(path, **kwargs)
        _raise_for_status(resp)
        return resp.json()

    def get_bytes(self, path: str, **kwargs) -> bytes:
        resp = self._client.get(path, **kwargs)
        _raise_for_status(resp)
        return resp.content

    def put(self, path: str, **kwargs) -> dict:
        resp = self._client.put(path, **kwargs)
        _raise_for_status(resp)
        return resp.json()

    def delete(self, path: str, **kwargs) -> dict | None:
        resp = self._client.delete(path, **kwargs)
        _raise_for_status(resp)
        if resp.status_code == 204:
            return None
        return resp.json()

    def ws_url(self, path: str) -> str:
        """Convert HTTP URL to WebSocket URL."""
        return self.base_url.replace("http://", "ws://").replace("https://", "wss://") + path

    # ── Client-level API methods ────────────────────────────────

    def list_sandboxes(self) -> list[dict]:
        """List all active sandboxes."""
        return self.get("/sandboxes")

    def pool_status(self) -> dict:
        """Get pool status (warm VMs, capacity)."""
        return self.get("/pool/status")

    def health(self) -> dict:
        """Health check (public, no auth required)."""
        return self.get("/health")

    def close(self):
        self._client.close()
