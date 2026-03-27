import os
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
