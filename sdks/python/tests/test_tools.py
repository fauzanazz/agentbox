import json
from unittest.mock import MagicMock

import pytest

from agentbox.tools import SANDBOX_TOOLS, get_tool_definitions, handle_tool_call
from agentbox.types import ExecResult


# ── Format tests (unchanged) ───────────────────────────────────


def test_openai_format():
    tools = get_tool_definitions("openai")
    assert len(tools) == len(SANDBOX_TOOLS)
    for t in tools:
        assert t["type"] == "function"
        assert "function" in t
        assert "name" in t["function"]
        assert "description" in t["function"]
        assert "parameters" in t["function"]
        assert t["function"]["parameters"]["type"] == "object"
        assert "properties" in t["function"]["parameters"]
        assert "required" in t["function"]["parameters"]


def test_anthropic_format():
    tools = get_tool_definitions("anthropic")
    assert len(tools) == len(SANDBOX_TOOLS)
    for t in tools:
        assert "name" in t
        assert "description" in t
        assert "input_schema" in t
        assert t["input_schema"]["type"] == "object"
        assert "properties" in t["input_schema"]
        assert "required" in t["input_schema"]


def test_generic_format():
    tools = get_tool_definitions("generic")
    assert tools == SANDBOX_TOOLS


# ── handle_tool_call (raise_on_error=True, default) ────────────


def test_handle_tool_call_openai_execute():
    sandbox = MagicMock()
    sandbox.exec.return_value = ExecResult(stdout="ok\n", stderr="", exit_code=0)

    result = handle_tool_call(sandbox, {
        "function": {
            "name": "execute_code",
            "arguments": json.dumps({"command": "echo ok"}),
        }
    })
    assert result["stdout"] == "ok\n"
    assert result["exit_code"] == 0
    sandbox.exec.assert_called_once_with("echo ok")


def test_handle_tool_call_anthropic_execute():
    sandbox = MagicMock()
    sandbox.exec.return_value = ExecResult(stdout="hi\n", stderr="", exit_code=0)

    result = handle_tool_call(sandbox, {
        "name": "execute_code",
        "input": {"command": "echo hi"},
    })
    assert result["stdout"] == "hi\n"
    sandbox.exec.assert_called_once_with("echo hi")


def test_handle_tool_call_write_file():
    sandbox = MagicMock()

    result = handle_tool_call(sandbox, {
        "name": "write_file",
        "input": {"path": "/workspace/test.py", "content": "print('hello')"},
    })
    assert result["status"] == "written"
    assert result["path"] == "/workspace/test.py"
    sandbox.upload_content.assert_called_once_with(b"print('hello')", "/workspace/test.py")


def test_handle_tool_call_read_file():
    sandbox = MagicMock()
    sandbox.download.return_value = b"file contents"

    result = handle_tool_call(sandbox, {
        "name": "read_file",
        "input": {"path": "/workspace/data.txt"},
    })
    assert result["content"] == "file contents"
    sandbox.download.assert_called_once_with("/workspace/data.txt")


def test_handle_tool_call_openai_string_args():
    sandbox = MagicMock()
    sandbox.exec.return_value = ExecResult(stdout="", stderr="", exit_code=0)

    result = handle_tool_call(sandbox, {
        "function": {
            "name": "execute_code",
            "arguments": '{"command": "ls"}',
        }
    })
    assert result["exit_code"] == 0
    sandbox.exec.assert_called_once_with("ls")


# ── raise_on_error=True (default): exceptions ──────────────────


def test_handle_tool_call_unknown_raises():
    sandbox = MagicMock()
    with pytest.raises(ValueError, match="Unknown tool"):
        handle_tool_call(sandbox, {
            "name": "unknown_tool",
            "input": {"foo": "bar"},
        })


def test_handle_tool_call_invalid_raises():
    sandbox = MagicMock()
    with pytest.raises(ValueError, match="Could not parse"):
        handle_tool_call(sandbox, {})


def test_handle_tool_call_invalid_json_raises():
    sandbox = MagicMock()
    with pytest.raises(ValueError, match="Invalid JSON"):
        handle_tool_call(sandbox, {
            "function": {
                "name": "execute_code",
                "arguments": "not valid json{{{",
            }
        })


def test_handle_tool_call_missing_required_raises():
    sandbox = MagicMock()
    with pytest.raises(ValueError, match="Missing required parameter"):
        handle_tool_call(sandbox, {
            "name": "execute_code",
            "input": {"wrong_key": "value"},
        })


def test_handle_tool_call_write_file_missing_params_raises():
    sandbox = MagicMock()
    with pytest.raises(ValueError, match="content"):
        handle_tool_call(sandbox, {
            "name": "write_file",
            "input": {"path": "/workspace/test.py"},
        })


def test_handle_tool_call_write_file_non_string_content_raises():
    sandbox = MagicMock()
    with pytest.raises(ValueError, match="content"):
        handle_tool_call(sandbox, {
            "name": "write_file",
            "input": {"path": "/workspace/test.py", "content": 12345},
        })
    sandbox.upload_content.assert_not_called()


# ── raise_on_error=False (legacy): error dicts ─────────────────


def test_handle_tool_call_unknown_legacy():
    sandbox = MagicMock()
    result = handle_tool_call(sandbox, {
        "name": "unknown_tool",
        "input": {"foo": "bar"},
    }, raise_on_error=False)
    assert "error" in result
    assert "Unknown tool" in result["error"]


def test_handle_tool_call_invalid_legacy():
    sandbox = MagicMock()
    result = handle_tool_call(sandbox, {}, raise_on_error=False)
    assert "error" in result


def test_handle_tool_call_invalid_json_legacy():
    sandbox = MagicMock()
    result = handle_tool_call(sandbox, {
        "function": {
            "name": "execute_code",
            "arguments": "not valid json{{{",
        }
    }, raise_on_error=False)
    assert "error" in result
    assert "Invalid JSON" in result["error"]


def test_handle_tool_call_missing_params_legacy():
    sandbox = MagicMock()
    result = handle_tool_call(sandbox, {
        "name": "execute_code",
        "input": {"wrong_key": "value"},
    }, raise_on_error=False)
    assert "error" in result
    assert "Missing required parameter" in result["error"]
