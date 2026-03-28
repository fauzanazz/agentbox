import json

import httpx
import respx

from agentbox import Sandbox
from agentbox.client import AgentBoxClient
from agentbox.errors import AgentBoxAPIError, SandboxNotFoundError
from agentbox.types import ExecResult, FileEntry, PortForwardInfo, SandboxInfo


# ── Create ──────────────────────────────────────────────────────


@respx.mock
def test_sandbox_create():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-abc"})
    )
    sb = Sandbox.create()
    assert sb.id == "sb-abc"
    sb._client.close()


@respx.mock
def test_sandbox_create_with_options():
    route = respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-custom"})
    )
    sb = Sandbox.create(memory_mb=4096, vcpus=4, network=True, timeout=7200)
    assert sb.id == "sb-custom"

    body = json.loads(route.calls[0].request.content)
    assert body["memory_mb"] == 4096
    assert body["vcpus"] == 4
    assert body["network"] is True
    assert body["timeout"] == 7200
    sb._client.close()


@respx.mock
def test_sandbox_create_with_api_key():
    route = respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-auth"})
    )
    sb = Sandbox.create(api_key="my-key")
    assert sb._client.api_key == "my-key"
    assert route.calls[0].request.headers["Authorization"] == "Bearer my-key"
    sb._client.close()


# ── Exec ────────────────────────────────────────────────────────


@respx.mock
def test_sandbox_exec():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-exec"})
    )
    respx.post("http://localhost:8080/sandboxes/sb-exec/exec").mock(
        return_value=httpx.Response(
            200, json={"stdout": "hello\n", "stderr": "", "exit_code": 0}
        )
    )
    sb = Sandbox.create()
    result = sb.exec("echo hello")
    assert isinstance(result, ExecResult)
    assert result.stdout == "hello\n"
    assert result.exit_code == 0
    sb._client.close()


# ── File operations ─────────────────────────────────────────────


@respx.mock
def test_sandbox_upload(tmp_path):
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-upload"})
    )
    respx.post("http://localhost:8080/sandboxes/sb-upload/files").mock(
        return_value=httpx.Response(200, json={"path": "/workspace/test.txt", "size": 5})
    )
    test_file = tmp_path / "test.txt"
    test_file.write_text("hello")

    sb = Sandbox.create()
    sb.upload(str(test_file), "/workspace/test.txt")
    sb._client.close()


@respx.mock
def test_sandbox_upload_content():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-upload"})
    )
    respx.post("http://localhost:8080/sandboxes/sb-upload/files").mock(
        return_value=httpx.Response(200, json={"path": "/workspace/test.txt", "size": 5})
    )
    sb = Sandbox.create()
    sb.upload_content(b"hello", "/workspace/test.txt")
    sb._client.close()


@respx.mock
def test_sandbox_download():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-dl"})
    )
    respx.get("http://localhost:8080/sandboxes/sb-dl/files").mock(
        return_value=httpx.Response(200, content=b"file data")
    )
    sb = Sandbox.create()
    data = sb.download("/workspace/output.txt")
    assert data == b"file data"
    sb._client.close()


@respx.mock
def test_sandbox_list_files():
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
    sb = Sandbox.create()
    files = sb.list_files()
    assert len(files) == 2
    assert isinstance(files[0], FileEntry)
    assert files[0].name == "script.py"
    assert files[1].is_dir is True
    sb._client.close()


@respx.mock
def test_sandbox_delete_file():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-del"})
    )
    route = respx.delete("http://localhost:8080/sandboxes/sb-del/files").mock(
        return_value=httpx.Response(200, json={"status": "deleted", "path": "/workspace/old.txt"})
    )
    sb = Sandbox.create()
    sb.delete_file("/workspace/old.txt")
    assert "path" in str(route.calls[0].request.url)
    sb._client.close()


@respx.mock
def test_sandbox_mkdir():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-mk"})
    )
    route = respx.put("http://localhost:8080/sandboxes/sb-mk/files").mock(
        return_value=httpx.Response(201, json={"status": "created", "path": "/workspace/newdir"})
    )
    sb = Sandbox.create()
    sb.mkdir("/workspace/newdir")
    assert "path" in str(route.calls[0].request.url)
    sb._client.close()


# ── Signals ─────────────────────────────────────────────────────


@respx.mock
def test_sandbox_send_signal():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-sig"})
    )
    route = respx.post("http://localhost:8080/sandboxes/sb-sig/signal").mock(
        return_value=httpx.Response(200, json={"status": "signal_sent", "signal": 9})
    )
    sb = Sandbox.create()
    sb.send_signal(9)
    body = json.loads(route.calls[0].request.content)
    assert body["signal"] == 9
    sb._client.close()


# ── Port forwarding ────────────────────────────────────────────


@respx.mock
def test_sandbox_port_forward():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-pf"})
    )
    respx.post("http://localhost:8080/sandboxes/sb-pf/ports").mock(
        return_value=httpx.Response(
            201, json={"guest_port": 3000, "host_port": 49152, "local_address": "0.0.0.0:49152"}
        )
    )
    sb = Sandbox.create()
    pf = sb.port_forward(3000)
    assert isinstance(pf, PortForwardInfo)
    assert pf.guest_port == 3000
    assert pf.host_port == 49152
    sb._client.close()


@respx.mock
def test_sandbox_list_port_forwards():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-pf"})
    )
    respx.get("http://localhost:8080/sandboxes/sb-pf/ports").mock(
        return_value=httpx.Response(
            200,
            json={
                "ports": [
                    {"guest_port": 3000, "host_port": 49152, "local_address": "0.0.0.0:49152"},
                    {"guest_port": 8080, "host_port": 49153, "local_address": "0.0.0.0:49153"},
                ]
            },
        )
    )
    sb = Sandbox.create()
    ports = sb.list_port_forwards()
    assert len(ports) == 2
    assert ports[0].guest_port == 3000
    assert ports[1].guest_port == 8080
    sb._client.close()


@respx.mock
def test_sandbox_remove_port_forward():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-pf"})
    )
    respx.delete("http://localhost:8080/sandboxes/sb-pf/ports/3000").mock(
        return_value=httpx.Response(200, json={"status": "removed"})
    )
    sb = Sandbox.create()
    sb.remove_port_forward(3000)
    sb._client.close()


# ── Info & lifecycle ────────────────────────────────────────────


@respx.mock
def test_sandbox_info():
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
    sb = Sandbox.create()
    info = sb.info()
    assert isinstance(info, SandboxInfo)
    assert info.status == "running"
    sb._client.close()


@respx.mock
def test_sandbox_destroy():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-destroy"})
    )
    respx.delete("http://localhost:8080/sandboxes/sb-destroy").mock(
        return_value=httpx.Response(200, json={"status": "destroyed"})
    )
    sb = Sandbox.create()
    sb.destroy()  # Should not raise


@respx.mock
def test_sandbox_context_manager():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-ctx"})
    )
    respx.delete("http://localhost:8080/sandboxes/sb-ctx").mock(
        return_value=httpx.Response(200, json={"status": "destroyed"})
    )
    with Sandbox.create() as sb:
        assert sb.id == "sb-ctx"
    # destroy was called automatically via __exit__


@respx.mock
def test_sandbox_exit_raises_destroy_errors_on_clean_exit():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-err"})
    )
    respx.delete("http://localhost:8080/sandboxes/sb-err").mock(
        return_value=httpx.Response(500, json={"error": "internal"})
    )
    # Destroy error should propagate when with-block exits cleanly
    try:
        with Sandbox.create() as sb:
            assert sb.id == "sb-err"
    except AgentBoxAPIError:
        pass  # Expected: destroy error propagates
    else:
        raise AssertionError("Expected AgentBoxAPIError from failed destroy")


@respx.mock
def test_sandbox_exit_does_not_mask_user_exception():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-mask"})
    )
    respx.delete("http://localhost:8080/sandboxes/sb-mask").mock(
        return_value=httpx.Response(500, json={"error": "internal"})
    )
    # The user's ValueError should propagate, not be masked by destroy failure
    try:
        with Sandbox.create() as sb:
            raise ValueError("user error")
    except ValueError as e:
        assert str(e) == "user error"
    else:
        raise AssertionError("Expected ValueError to propagate")


# ── Error cases ─────────────────────────────────────────────────


@respx.mock
def test_sandbox_not_found_error():
    respx.post("http://localhost:8080/sandboxes").mock(
        return_value=httpx.Response(200, json={"id": "sb-gone"})
    )
    respx.get("http://localhost:8080/sandboxes/sb-gone").mock(
        return_value=httpx.Response(404, json={"error": "Sandbox not found"})
    )
    import pytest
    sb = Sandbox.create()
    with pytest.raises(SandboxNotFoundError):
        sb.info()
    sb._client.close()
