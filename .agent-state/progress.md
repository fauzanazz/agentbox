# Progress — FAU-74 Python SDK (Revision)

## Accomplished

Addressed all 6 review violations from cubic-dev-ai:

1. **`__exit__` error suppression** (`sandbox.py`) — Wrapped `self.destroy()` in try/except so destroy errors don't mask exceptions raised inside `with` blocks.

2. **JSON parsing guard** (`tools.py`) — Wrapped `json.loads(args)` in try/except for `JSONDecodeError`/`TypeError`, returning a clear error dict instead of crashing.

3. **Args validation before dispatch** (`tools.py`) — Changed `not args` to `not isinstance(args, dict)` and added per-tool required parameter checks before accessing keys.

4. **POST body assertion in test** (`test_sandbox.py`) — `test_sandbox_create_with_options` now verifies `memory_mb`, `vcpus`, `network`, and `timeout` are sent in the POST body.

5. **README context manager** (`README.md`) — LLM integration example now uses `with Sandbox.create() as sb:` to prevent sandbox leaks.

6. **Design doc updates** (`docs/designs/python-sdk.md`) — Applied matching fixes for `__exit__` and `handle_tool_call` in the design doc.

Added new tests:
- `test_sandbox_exit_suppresses_destroy_errors`
- `test_sandbox_exit_does_not_mask_user_exception`
- `test_handle_tool_call_invalid_json_args`
- `test_handle_tool_call_missing_required_params`
- `test_handle_tool_call_write_file_missing_params`

## Test Results

41/41 tests passing.

## What's Left

Nothing — all review violations addressed, all tests pass.

## Decisions

- Empty dict `{}` as tool `input` is correctly caught by the existing `or` chain (falsy) and reported as unparseable — this is acceptable behavior since a valid tool call always has at least one required parameter.
