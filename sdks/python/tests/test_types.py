from agentbox.types import ExecResult, FileEntry, SandboxInfo


def test_exec_result_creation():
    result = ExecResult(stdout="hello\n", stderr="", exit_code=0)
    assert result.stdout == "hello\n"
    assert result.stderr == ""
    assert result.exit_code == 0


def test_exec_result_nonzero_exit():
    result = ExecResult(stdout="", stderr="error\n", exit_code=1)
    assert result.exit_code == 1
    assert result.stderr == "error\n"


def test_file_entry_creation():
    entry = FileEntry(name="test.py", size=1024, is_dir=False)
    assert entry.name == "test.py"
    assert entry.size == 1024
    assert entry.is_dir is False


def test_file_entry_directory():
    entry = FileEntry(name="src", size=4096, is_dir=True)
    assert entry.is_dir is True


def test_sandbox_info_creation():
    info = SandboxInfo(
        id="sb-123",
        status="running",
        config={"memory_mb": 2048, "vcpus": 2},
        created_at="2024-01-01T00:00:00Z",
    )
    assert info.id == "sb-123"
    assert info.status == "running"
    assert info.config["memory_mb"] == 2048
    assert info.created_at == "2024-01-01T00:00:00Z"
