import os
from unittest.mock import patch

import httpx
import pytest
import respx

from agentbox.client import AgentBoxClient


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
def test_client_delete():
    respx.delete("http://localhost:8080/sandboxes/sb-123").mock(
        return_value=httpx.Response(200, json={"status": "destroyed"})
    )
    client = AgentBoxClient()
    result = client.delete("/sandboxes/sb-123")
    assert result == {"status": "destroyed"}
    client.close()


@respx.mock
def test_client_error_handling():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(500, json={"error": "internal"})
    )
    client = AgentBoxClient()
    with pytest.raises(httpx.HTTPStatusError):
        client.post("/sandboxes", json={})
    client.close()


def test_client_ws_url():
    client = AgentBoxClient("http://localhost:8080")
    assert client.ws_url("/sandboxes/sb-123/ws") == "ws://localhost:8080/sandboxes/sb-123/ws"
    client.close()


def test_client_ws_url_https():
    client = AgentBoxClient("https://secure.example.com")
    assert client.ws_url("/ws") == "wss://secure.example.com/ws"
    client.close()
