import os
from unittest.mock import patch

import httpx
import pytest
import respx

from agentbox.async_client import AsyncAgentBoxClient
from agentbox.errors import AuthenticationError, AgentBoxAPIError


# ── URL resolution ──────────────────────────────────────────────


def test_async_client_default_url():
    with patch.dict(os.environ, {}, clear=True):
        os.environ.pop("AGENTBOX_URL", None)
        client = AsyncAgentBoxClient()
        assert client.base_url == "http://localhost:8080"


def test_async_client_explicit_url():
    client = AsyncAgentBoxClient("http://myhost:9090")
    assert client.base_url == "http://myhost:9090"


def test_async_client_env_url():
    with patch.dict(os.environ, {"AGENTBOX_URL": "http://env-host:3000"}):
        client = AsyncAgentBoxClient()
        assert client.base_url == "http://env-host:3000"


# ── Auth ────────────────────────────────────────────────────────


def test_async_client_no_auth_by_default():
    with patch.dict(os.environ, {}, clear=True):
        os.environ.pop("AGENTBOX_API_KEY", None)
        client = AsyncAgentBoxClient()
        assert client.api_key is None


def test_async_client_explicit_api_key():
    client = AsyncAgentBoxClient(api_key="my-secret-key")
    assert client.api_key == "my-secret-key"
    assert client._client.headers["Authorization"] == "Bearer my-secret-key"


def test_async_client_env_api_key():
    with patch.dict(os.environ, {"AGENTBOX_API_KEY": "env-key"}):
        client = AsyncAgentBoxClient()
        assert client.api_key == "env-key"


# ── HTTP methods ────────────────────────────────────────────────


@pytest.mark.asyncio
@respx.mock
async def test_async_client_post():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-123"})
    )
    client = AsyncAgentBoxClient()
    result = await client.post("/sandboxes", json={"memory_mb": 2048})
    assert result == {"id": "sb-123"}
    await client.close()


@pytest.mark.asyncio
@respx.mock
async def test_async_client_get():
    respx.get("http://localhost:8080/sandboxes/sb-123").mock(
        return_value=httpx.Response(200, json={"id": "sb-123", "status": "running"})
    )
    client = AsyncAgentBoxClient()
    result = await client.get("/sandboxes/sb-123")
    assert result["status"] == "running"
    await client.close()


@pytest.mark.asyncio
@respx.mock
async def test_async_client_get_bytes():
    respx.get("http://localhost:8080/sandboxes/sb-123/files").mock(
        return_value=httpx.Response(200, content=b"file content")
    )
    client = AsyncAgentBoxClient()
    result = await client.get_bytes("/sandboxes/sb-123/files", params={"path": "/test.txt"})
    assert result == b"file content"
    await client.close()


@pytest.mark.asyncio
@respx.mock
async def test_async_client_put():
    respx.put("http://localhost:8080/sandboxes/sb-123/files").mock(
        return_value=httpx.Response(201, json={"status": "created", "path": "/workspace/dir"})
    )
    client = AsyncAgentBoxClient()
    result = await client.put("/sandboxes/sb-123/files", params={"path": "/workspace/dir"})
    assert result["status"] == "created"
    await client.close()


@pytest.mark.asyncio
@respx.mock
async def test_async_client_delete():
    respx.delete("http://localhost:8080/sandboxes/sb-123").mock(
        return_value=httpx.Response(200, json={"status": "destroyed"})
    )
    client = AsyncAgentBoxClient()
    result = await client.delete("/sandboxes/sb-123")
    assert result == {"status": "destroyed"}
    await client.close()


# ── Error mapping ───────────────────────────────────────────────


@pytest.mark.asyncio
@respx.mock
async def test_async_client_401_raises():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(401, json={"error": "Invalid or missing API key"})
    )
    client = AsyncAgentBoxClient()
    with pytest.raises(AuthenticationError):
        await client.post("/sandboxes", json={})
    await client.close()


@pytest.mark.asyncio
@respx.mock
async def test_async_client_500_raises():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(500, json={"error": "internal"})
    )
    client = AsyncAgentBoxClient()
    with pytest.raises(AgentBoxAPIError):
        await client.post("/sandboxes", json={})
    await client.close()


# ── WebSocket URL ───────────────────────────────────────────────


def test_async_client_ws_url():
    client = AsyncAgentBoxClient("http://localhost:8080")
    assert client.ws_url("/sandboxes/sb-123/ws") == "ws://localhost:8080/sandboxes/sb-123/ws"


def test_async_client_ws_url_https():
    client = AsyncAgentBoxClient("https://secure.example.com")
    assert client.ws_url("/ws") == "wss://secure.example.com/ws"


# ── Client-level API methods ───────────────────────────────────


@pytest.mark.asyncio
@respx.mock
async def test_async_client_list_sandboxes():
    respx.get("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json=[{"id": "sb-1"}, {"id": "sb-2"}])
    )
    client = AsyncAgentBoxClient()
    result = await client.list_sandboxes()
    assert len(result) == 2
    await client.close()


@pytest.mark.asyncio
@respx.mock
async def test_async_client_pool_status():
    respx.get("http://localhost:8080/pool/status").mock(
        return_value=httpx.Response(200, json={"available": 3, "active": 1})
    )
    client = AsyncAgentBoxClient()
    result = await client.pool_status()
    assert result["available"] == 3
    await client.close()


@pytest.mark.asyncio
@respx.mock
async def test_async_client_health():
    respx.get("http://localhost:8080/health").mock(
        return_value=httpx.Response(200, json={"status": "ok"})
    )
    client = AsyncAgentBoxClient()
    result = await client.health()
    assert result["status"] == "ok"
    await client.close()
