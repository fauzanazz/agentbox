from unittest.mock import MagicMock

import pytest

from agentbox.errors import (
    AgentBoxAPIError,
    AgentBoxError,
    AuthenticationError,
    PathTraversalError,
    PoolExhaustedError,
    SandboxInUseError,
    SandboxNotFoundError,
    _raise_for_status,
)


def _mock_response(status_code: int, json_body: dict | None = None, text: str = ""):
    resp = MagicMock()
    resp.status_code = status_code
    resp.is_success = 200 <= status_code < 300
    resp.text = text or (str(json_body) if json_body else "")
    if json_body is not None:
        resp.json.return_value = json_body
    else:
        resp.json.side_effect = ValueError("No JSON")
    return resp


# ── Hierarchy ────────────────────────────────────────────────────


def test_all_errors_inherit_from_base():
    for cls in (
        AuthenticationError,
        SandboxNotFoundError,
        PathTraversalError,
        SandboxInUseError,
        PoolExhaustedError,
        AgentBoxAPIError,
    ):
        assert issubclass(cls, AgentBoxError)
        assert issubclass(cls, Exception)


def test_base_error_attributes():
    err = AgentBoxError("boom", status_code=500, response_body='{"error":"boom"}')
    assert str(err) == "boom"
    assert err.status_code == 500
    assert err.response_body == '{"error":"boom"}'


def test_base_error_optional_attrs():
    err = AgentBoxError("simple")
    assert err.status_code is None
    assert err.response_body is None


# ── _raise_for_status mapping ───────────────────────────────────


def test_success_does_not_raise():
    _raise_for_status(_mock_response(200))
    _raise_for_status(_mock_response(201))


def test_401_raises_authentication_error():
    resp = _mock_response(401, {"error": "Invalid or missing API key"})
    with pytest.raises(AuthenticationError) as exc_info:
        _raise_for_status(resp)
    assert exc_info.value.status_code == 401
    assert "API key" in str(exc_info.value)


def test_404_raises_sandbox_not_found():
    resp = _mock_response(404, {"error": "Sandbox not found"})
    with pytest.raises(SandboxNotFoundError) as exc_info:
        _raise_for_status(resp)
    assert exc_info.value.status_code == 404


def test_503_raises_pool_exhausted():
    resp = _mock_response(503, {"error": "Pool exhausted"})
    with pytest.raises(PoolExhaustedError) as exc_info:
        _raise_for_status(resp)
    assert exc_info.value.status_code == 503


def test_400_path_traversal():
    resp = _mock_response(400, {"error": "Path traversal: escapes /workspace"})
    with pytest.raises(PathTraversalError):
        _raise_for_status(resp)


def test_400_sandbox_in_use():
    resp = _mock_response(400, {"error": "Sandbox is currently in use by another request"})
    with pytest.raises(SandboxInUseError):
        _raise_for_status(resp)


def test_400_generic_falls_to_api_error():
    resp = _mock_response(400, {"error": "guest_port must be > 0"})
    with pytest.raises(AgentBoxAPIError) as exc_info:
        _raise_for_status(resp)
    assert exc_info.value.status_code == 400


def test_500_raises_api_error():
    resp = _mock_response(500, {"error": "internal server error"})
    with pytest.raises(AgentBoxAPIError) as exc_info:
        _raise_for_status(resp)
    assert exc_info.value.status_code == 500


def test_non_json_error_body():
    resp = _mock_response(502, text="Bad Gateway")
    resp.json.side_effect = ValueError("No JSON")
    with pytest.raises(AgentBoxAPIError) as exc_info:
        _raise_for_status(resp)
    assert "Bad Gateway" in str(exc_info.value)
