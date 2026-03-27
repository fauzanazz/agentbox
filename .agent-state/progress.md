# Progress — FAU-74 Python SDK (Revision 2)

## Accomplished

Addressed remaining review feedback from cubic-dev-ai's second review:

1. **P1: `__exit__` conditional error suppression** (`sandbox.py`) — Changed `__exit__` to only suppress destroy errors when there's an active user exception. On clean exit, destroy errors now propagate so resource leaks are surfaced. Updated test `test_sandbox_exit_raises_destroy_errors_on_clean_exit` accordingly.

2. **P2: `write_file` content type validation** (`tools.py`) — Added `isinstance(content, str)` check before calling `.encode()`, returning a clear error dict for non-string content. Added `test_handle_tool_call_write_file_non_string_content` test.

## Test Results

42/42 tests passing.

## What's Left

Nothing — all review violations from both review rounds addressed, all tests pass.

## Decisions

- `__exit__` uses `if exc_type is None: raise` pattern — destroy errors propagate on clean exit but are suppressed when a user exception is active, preventing masking.
