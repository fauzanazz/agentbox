import json

import httpx
import pytest
import respx

from agentbox.async_sandbox import AsyncSandbox
from agentbox.errors import AgentBoxAPIError, SandboxNotFoundError
from agentbox.types import ExecResult, FileEntry, PortForwardInfo, SandboxInfo


# ── Create ──────────────────────────────────────────────────────


@pytest.mark.asyncio
@respx.mock
async def test_async_sandbox_create():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-abc"})
    )
    sb = await AsyncSandbox.create()
    assert sb.id == "sb-abc"
    await sb._client.close()


@pytest.mark.asyncio
@respx.mock
async def test_async_sandbox_create_with_options():
    route = respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-custom"})
    )
    sb = await AsyncSandbox.create(memory_mb=4096, vcpus=4, network=True, timeout=7200)
    assert sb.id == "sb-custom"
    body = json.loads(route.calls[0].request.content)
    assert body["memory_mb"] == 4096
    await sb._client.close()


@pytest.mark.asyncio
@respx.mock
async def test_async_sandbox_create_with_api_key():
    route = respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-auth"})
    )
    sb = await AsyncSandbox.create(api_key="my-key")
    assert sb._client.api_key == "my-key"
    assert route.calls[0].request.headers["Authorization"] == "Bearer my-key"
    await sb._client.close()


# ── Exec ────────────────────────────────────────────────────────


@pytest.mark.asyncio
@respx.mock
async def test_async_sandbox_exec():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-exec"})
    )
    respx.post("http://localhost:8080/sandboxes/sb-exec/exec").mock(
        return_value=httpx.Response(
            200, json={"stdout": "hello\n", "stderr": "", "exit_code": 0}
        )
    )
    sb = await AsyncSandbox.create()
    result = await sb.exec("echo hello")
    assert isinstance(result, ExecResult)
    assert result.stdout == "hello\n"
    assert result.exit_code == 0
    await sb._client.close()


# ── File operations ─────────────────────────────────────────────


@pytest.mark.asyncio
@respx.mock
async def test_async_sandbox_upload_content():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-upload"})
    )
    respx.post("http://localhost:8080/sandboxes/sb-upload/files").mock(
        return_value=httpx.Response(200, json={"path": "/workspace/test.txt", "size": 5})
    )
    sb = await AsyncSandbox.create()
    await sb.upload_content(b"hello", "/workspace/test.txt")
    await sb._client.close()


@pytest.mark.asyncio
@respx.mock
async def test_async_sandbox_download():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-dl"})
    )
    respx.get("http://localhost:8080/sandboxes/sb-dl/files").mock(
        return_value=httpx.Response(200, content=b"file data")
    )
    sb = await AsyncSandbox.create()
    data = await sb.download("/workspace/output.txt")
    assert data == b"file data"
    await sb._client.close()


@pytest.mark.asyncio
@respx.mock
async def test_async_sandbox_list_files():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-ls"})
    )
    respx.get("http://localhost:8080/sandboxes/sb-ls/files").mock(
        return_value=httpx.Response(
            200,
            json=[
                {"name": "script.py", "size": 256, "is_dir": False},
                {"name": "data", "size": 4096, "is_dir": True},
            ],
        )
    )
    sb = await AsyncSandbox.create()
    files = await sb.list_files()
    assert len(files) == 2
    assert isinstance(files[0], FileEntry)
    await sb._client.close()


@pytest.mark.asyncio
@respx.mock
async def test_async_sandbox_delete_file():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-del"})
    )
    respx.delete("http://localhost:8080/sandboxes/sb-del/files").mock(
        return_value=httpx.Response(200, json={"status": "deleted", "path": "/workspace/old.txt"})
    )
    sb = await AsyncSandbox.create()
    await sb.delete_file("/workspace/old.txt")
    await sb._client.close()


@pytest.mark.asyncio
@respx.mock
async def test_async_sandbox_mkdir():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-mk"})
    )
    respx.put("http://localhost:8080/sandboxes/sb-mk/files").mock(
        return_value=httpx.Response(201, json={"status": "created", "path": "/workspace/newdir"})
    )
    sb = await AsyncSandbox.create()
    await sb.mkdir("/workspace/newdir")
    await sb._client.close()


# ── Signals ─────────────────────────────────────────────────────


@pytest.mark.asyncio
@respx.mock
async def test_async_sandbox_send_signal():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-sig"})
    )
    route = respx.post("http://localhost:8080/sandboxes/sb-sig/signal").mock(
        return_value=httpx.Response(200, json={"status": "signal_sent", "signal": 9})
    )
    sb = await AsyncSandbox.create()
    await sb.send_signal(9)
    body = json.loads(route.calls[0].request.content)
    assert body["signal"] == 9
    await sb._client.close()


# ── Port forwarding ────────────────────────────────────────────


@pytest.mark.asyncio
@respx.mock
async def test_async_sandbox_port_forward():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-pf"})
    )
    respx.post("http://localhost:8080/sandboxes/sb-pf/ports").mock(
        return_value=httpx.Response(
            201, json={"guest_port": 3000, "host_port": 49152, "local_address": "0.0.0.0:49152"}
        )
    )
    sb = await AsyncSandbox.create()
    pf = await sb.port_forward(3000)
    assert isinstance(pf, PortForwardInfo)
    assert pf.guest_port == 3000
    await sb._client.close()


@pytest.mark.asyncio
@respx.mock
async def test_async_sandbox_list_port_forwards():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-pf"})
    )
    respx.get("http://localhost:8080/sandboxes/sb-pf/ports").mock(
        return_value=httpx.Response(
            200,
            json={"ports": [
                {"guest_port": 3000, "host_port": 49152, "local_address": "0.0.0.0:49152"},
            ]},
        )
    )
    sb = await AsyncSandbox.create()
    ports = await sb.list_port_forwards()
    assert len(ports) == 1
    await sb._client.close()


@pytest.mark.asyncio
@respx.mock
async def test_async_sandbox_remove_port_forward():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-pf"})
    )
    respx.delete("http://localhost:8080/sandboxes/sb-pf/ports/3000").mock(
        return_value=httpx.Response(200, json={"status": "removed"})
    )
    sb = await AsyncSandbox.create()
    await sb.remove_port_forward(3000)
    await sb._client.close()


# ── Info & lifecycle ────────────────────────────────────────────


@pytest.mark.asyncio
@respx.mock
async def test_async_sandbox_info():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-info"})
    )
    respx.get("http://localhost:8080/sandboxes/sb-info").mock(
        return_value=httpx.Response(
            200,
            json={
                "id": "sb-info",
                "status": "running",
                "config": {"memory_mb": 2048},
                "created_at": "2024-01-01T00:00:00Z",
            },
        )
    )
    sb = await AsyncSandbox.create()
    info = await sb.info()
    assert isinstance(info, SandboxInfo)
    assert info.status == "running"
    await sb._client.close()


@pytest.mark.asyncio
@respx.mock
async def test_async_sandbox_destroy():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-destroy"})
    )
    respx.delete("http://localhost:8080/sandboxes/sb-destroy").mock(
        return_value=httpx.Response(200, json={"status": "destroyed"})
    )
    sb = await AsyncSandbox.create()
    await sb.destroy()


@pytest.mark.asyncio
@respx.mock
async def test_async_sandbox_context_manager():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-ctx"})
    )
    respx.delete("http://localhost:8080/sandboxes/sb-ctx").mock(
        return_value=httpx.Response(200, json={"status": "destroyed"})
    )
    async with await AsyncSandbox.create() as sb:
        assert sb.id == "sb-ctx"


@pytest.mark.asyncio
@respx.mock
async def test_async_sandbox_exit_raises_destroy_errors_on_clean_exit():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-err"})
    )
    respx.delete("http://localhost:8080/sandboxes/sb-err").mock(
        return_value=httpx.Response(500, json={"error": "internal"})
    )
    try:
        async with await AsyncSandbox.create() as sb:
            assert sb.id == "sb-err"
    except AgentBoxAPIError:
        pass
    else:
        raise AssertionError("Expected AgentBoxAPIError from failed destroy")


@pytest.mark.asyncio
@respx.mock
async def test_async_sandbox_not_found():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-gone"})
    )
    respx.get("http://localhost:8080/sandboxes/sb-gone").mock(
        return_value=httpx.Response(404, json={"error": "Sandbox not found"})
    )
    sb = await AsyncSandbox.create()
    with pytest.raises(SandboxNotFoundError):
        await sb.info()
    await sb._client.close()
