import os
from unittest.mock import patch

import httpx
import pytest
import respx

from agentbox.client import AgentBoxClient
from agentbox.errors import (
    AgentBoxAPIError,
    AuthenticationError,
    PoolExhaustedError,
    SandboxNotFoundError,
)


# ── URL resolution ──────────────────────────────────────────────


def test_client_default_url():
    with patch.dict(os.environ, {}, clear=True):
        os.environ.pop("AGENTBOX_URL", None)
        client = AgentBoxClient()
        assert client.base_url == "http://localhost:8080"
        client.close()


def test_client_explicit_url():
    client = AgentBoxClient("http://myhost:9090")
    assert client.base_url == "http://myhost:9090"
    client.close()


def test_client_env_url():
    with patch.dict(os.environ, {"AGENTBOX_URL": "http://env-host:3000"}):
        client = AgentBoxClient()
        assert client.base_url == "http://env-host:3000"
        client.close()


def test_client_strips_trailing_slash():
    client = AgentBoxClient("http://localhost:8080/")
    assert client.base_url == "http://localhost:8080"
    client.close()


# ── Auth ────────────────────────────────────────────────────────


def test_client_no_auth_by_default():
    with patch.dict(os.environ, {}, clear=True):
        os.environ.pop("AGENTBOX_API_KEY", None)
        client = AgentBoxClient()
        assert client.api_key is None
        assert "Authorization" not in client._client.headers
        client.close()


def test_client_explicit_api_key():
    client = AgentBoxClient(api_key="my-secret-key")
    assert client.api_key == "my-secret-key"
    assert client._client.headers["Authorization"] == "Bearer my-secret-key"
    client.close()


def test_client_env_api_key():
    with patch.dict(os.environ, {"AGENTBOX_API_KEY": "env-key"}):
        client = AgentBoxClient()
        assert client.api_key == "env-key"
        assert client._client.headers["Authorization"] == "Bearer env-key"
        client.close()


def test_client_explicit_api_key_overrides_env():
    with patch.dict(os.environ, {"AGENTBOX_API_KEY": "env-key"}):
        client = AgentBoxClient(api_key="explicit-key")
        assert client.api_key == "explicit-key"
        client.close()


@respx.mock
def test_client_auth_header_sent():
    route = respx.get("http://localhost:8080/health").mock(
        return_value=httpx.Response(200, json={"status": "ok"})
    )
    client = AgentBoxClient(api_key="test-key")
    client.get("/health")
    assert route.calls[0].request.headers["Authorization"] == "Bearer test-key"
    client.close()


# ── HTTP methods ────────────────────────────────────────────────


@respx.mock
def test_client_post():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-123"})
    )
    client = AgentBoxClient()
    result = client.post("/sandboxes", json={"memory_mb": 2048})
    assert result == {"id": "sb-123"}
    client.close()


@respx.mock
def test_client_get():
    respx.get("http://localhost:8080/sandboxes/sb-123").mock(
        return_value=httpx.Response(200, json={"id": "sb-123", "status": "running"})
    )
    client = AgentBoxClient()
    result = client.get("/sandboxes/sb-123")
    assert result["status"] == "running"
    client.close()


@respx.mock
def test_client_get_bytes():
    respx.get("http://localhost:8080/sandboxes/sb-123/files").mock(
        return_value=httpx.Response(200, content=b"file content")
    )
    client = AgentBoxClient()
    result = client.get_bytes("/sandboxes/sb-123/files", params={"path": "/test.txt"})
    assert result == b"file content"
    client.close()


@respx.mock
def test_client_put():
    respx.put("http://localhost:8080/sandboxes/sb-123/files").mock(
        return_value=httpx.Response(201, json={"status": "created", "path": "/workspace/dir"})
    )
    client = AgentBoxClient()
    result = client.put("/sandboxes/sb-123/files", params={"path": "/workspace/dir"})
    assert result["status"] == "created"
    client.close()


@respx.mock
def test_client_delete():
    respx.delete("http://localhost:8080/sandboxes/sb-123").mock(
        return_value=httpx.Response(200, json={"status": "destroyed"})
    )
    client = AgentBoxClient()
    result = client.delete("/sandboxes/sb-123")
    assert result == {"status": "destroyed"}
    client.close()


# ── Error mapping ───────────────────────────────────────────────


@respx.mock
def test_client_401_raises_authentication_error():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(401, json={"error": "Invalid or missing API key"})
    )
    client = AgentBoxClient()
    with pytest.raises(AuthenticationError):
        client.post("/sandboxes", json={})
    client.close()


@respx.mock
def test_client_404_raises_not_found():
    respx.get("http://localhost:8080/sandboxes/sb-nope").mock(
        return_value=httpx.Response(404, json={"error": "Sandbox not found"})
    )
    client = AgentBoxClient()
    with pytest.raises(SandboxNotFoundError):
        client.get("/sandboxes/sb-nope")
    client.close()


@respx.mock
def test_client_503_raises_pool_exhausted():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(503, json={"error": "Pool exhausted"})
    )
    client = AgentBoxClient()
    with pytest.raises(PoolExhaustedError):
        client.post("/sandboxes", json={})
    client.close()


@respx.mock
def test_client_500_raises_api_error():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(500, json={"error": "internal"})
    )
    client = AgentBoxClient()
    with pytest.raises(AgentBoxAPIError):
        client.post("/sandboxes", json={})
    client.close()


# ── WebSocket URL ───────────────────────────────────────────────


def test_client_ws_url():
    client = AgentBoxClient("http://localhost:8080")
    assert client.ws_url("/sandboxes/sb-123/ws") == "ws://localhost:8080/sandboxes/sb-123/ws"
    client.close()


def test_client_ws_url_https():
    client = AgentBoxClient("https://secure.example.com")
    assert client.ws_url("/ws") == "wss://secure.example.com/ws"
    client.close()


# ── Client-level API methods ───────────────────────────────────


@respx.mock
def test_client_list_sandboxes():
    respx.get("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json=[{"id": "sb-1"}, {"id": "sb-2"}])
    )
    client = AgentBoxClient()
    result = client.list_sandboxes()
    assert len(result) == 2
    assert result[0]["id"] == "sb-1"
    client.close()


@respx.mock
def test_client_pool_status():
    respx.get("http://localhost:8080/pool/status").mock(
        return_value=httpx.Response(200, json={"available": 3, "active": 1})
    )
    client = AgentBoxClient()
    result = client.pool_status()
    assert result["available"] == 3
    client.close()


@respx.mock
def test_client_health():
    respx.get("http://localhost:8080/health").mock(
        return_value=httpx.Response(200, json={"status": "ok", "pool": {"active": 0, "max_size": 5}})
    )
    client = AgentBoxClient()
    result = client.health()
    assert result["status"] == "ok"
    client.close()
