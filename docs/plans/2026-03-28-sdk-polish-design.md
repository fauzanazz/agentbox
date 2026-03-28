# Plan: SDK Polish — Full Parity + Async Python + Auth + Tests

Multi-session plan, sliced by SDK. Each session ends green with failing tests for the next.

---

## Session 1: Python SDK

### P1. Error Hierarchy
**New file:** `sdks/python/agentbox/errors.py`
- `AgentBoxError(Exception)` — base, stores `message`, optional `status_code`, optional `response_body`
- `AuthenticationError(AgentBoxError)` — 401
- `SandboxNotFoundError(AgentBoxError)` — 404
- `PathTraversalError(AgentBoxError)` — 400 (when response body contains "path" hints)
- `SandboxInUseError(AgentBoxError)` — 400 (when response body contains "in use")
- `PoolExhaustedError(AgentBoxError)` — 503
- `AgentBoxAPIError(AgentBoxError)` — catch-all for other HTTP errors

**Modify:** `sdks/python/agentbox/client.py`
- Replace raw `resp.raise_for_status()` with custom error mapping:
  - Parse response body for error message
  - Map status code → appropriate exception subclass
  - 400 → inspect body to distinguish PathTraversal vs SandboxInUse vs generic BadRequest
  - 401 → AuthenticationError
  - 404 → SandboxNotFoundError
  - 503 → PoolExhaustedError
  - Other → AgentBoxAPIError

### P2. Auth Support
**Modify:** `sdks/python/agentbox/client.py`
- Add `api_key: str | None = None` parameter to `AgentBoxClient.__init__`
- Resolve from: explicit arg → `AGENTBOX_API_KEY` env var → None
- If set, add `Authorization: Bearer <key>` to all requests via `httpx.Client(headers=...)`
- Store `api_key` for WS connections

**Modify:** `sdks/python/agentbox/sandbox.py`
- Add `api_key: str | None = None` parameter to `Sandbox.create()`
- Pass through to `AgentBoxClient`
- Pass `extra_headers={"Authorization": f"Bearer {api_key}"}` to `websockets.connect()` in `exec_stream()`

### P3. Missing Methods on Sandbox
**Modify:** `sdks/python/agentbox/sandbox.py`
- `delete_file(path: str) -> None` — `DELETE /sandboxes/{id}/files?path=...`
- `mkdir(path: str) -> None` — `PUT /sandboxes/{id}/files?path=...`
- `send_signal(signal: int) -> None` — `POST /sandboxes/{id}/signal` with `{"signal": N}`
- `port_forward(guest_port: int) -> PortForwardInfo` — `POST /sandboxes/{id}/ports`
- `list_port_forwards() -> list[PortForwardInfo]` — `GET /sandboxes/{id}/ports`
- `remove_port_forward(guest_port: int) -> None` — `DELETE /sandboxes/{id}/ports/{guest_port}`

**Modify:** `sdks/python/agentbox/client.py`
- `put(path, **kwargs) -> dict` — new HTTP method for mkdir

**Modify:** `sdks/python/agentbox/types.py`
- Add `PortForwardInfo(BaseModel)`: `guest_port: int`, `host_port: int`, `local_address: str`

### P4. Client-Level Methods
These operate outside sandbox scope, so they go on `AgentBoxClient` (and later `AsyncAgentBoxClient`).

**Modify:** `sdks/python/agentbox/client.py`
- `list_sandboxes() -> list[dict]` — `GET /sandboxes`
- `pool_status() -> dict` — `GET /pool/status`
- `health() -> dict` — `GET /health`

### P5. WebSocket Stdin Support (new method, no breaking change)
**Modify:** `sdks/python/agentbox/sandbox.py`
- Keep `exec_stream(command)` as-is (returns `AsyncIterator[dict]`)
- Add new `exec_interactive(command)` that returns `ExecSession`

**New class in sandbox.py (or separate file):** `ExecSession`
- `events() -> AsyncIterator[dict]` — yields stdout/stderr/exit/error
- `async send_stdin(data: bytes) -> None` — sends base64-encoded stdin via WS
- `async send_signal(signal: int) -> None` — sends signal over WS
- `async close() -> None` — closes WS
- Async context manager (`__aenter__` / `__aexit__`)

### P6. AsyncSandbox + AsyncAgentBoxClient
**New file:** `sdks/python/agentbox/async_client.py`
- `AsyncAgentBoxClient` — mirrors `AgentBoxClient` using `httpx.AsyncClient`
- Same methods but all async: `post`, `get`, `get_bytes`, `delete`, `put`, `close`
- Same auth support (api_key parameter)
- Same error mapping
- `list_sandboxes()`, `pool_status()`, `health()` — async versions

**New file:** `sdks/python/agentbox/async_sandbox.py`
- `AsyncSandbox` — mirrors `Sandbox` but all methods async
- `await AsyncSandbox.create(...)` — async class method
- All sandbox methods: `exec`, `upload`, `upload_content`, `download`, `list_files`, `delete_file`, `mkdir`, `send_signal`, `info`, `destroy`
- Port forwarding: `port_forward`, `list_port_forwards`, `remove_port_forward`
- `exec_stream` returns `ExecSession` (same as P5)
- `__aenter__` / `__aexit__` context manager
- `tool_definitions()` and `handle_tool_call()` (async)

### P7. Tool Fixes (backward-compatible)
**Modify:** `sdks/python/agentbox/tools.py`
- Add `raise_on_error: bool = True` parameter to `handle_tool_call`
- When `raise_on_error=True` (default): raises exceptions on failure
  - Unknown tool → raises `ValueError`
  - Bad args → raises `ValueError`
  - Sandbox errors → raises `AgentBoxError` subclasses
- When `raise_on_error=False`: returns `{"error": ...}` dicts (old behavior)

### P8. Export Fixes
**Modify:** `sdks/python/agentbox/__init__.py`
- Add exports: `AgentBoxClient`, `AsyncSandbox`, `AsyncAgentBoxClient`, `get_tool_definitions`, `handle_tool_call`
- Add all error classes: `AgentBoxError`, `AuthenticationError`, `SandboxNotFoundError`, `PathTraversalError`, `SandboxInUseError`, `PoolExhaustedError`, `AgentBoxAPIError`
- Add `PortForwardInfo`, `ExecSession`

### P9. Tests
**New/modify test files:**
- `tests/test_errors.py` — error hierarchy, correct mapping from status codes
- `tests/test_client.py` — add auth tests (with/without key, env var), new methods (list_sandboxes, pool_status, health), put method
- `tests/test_sandbox.py` — add tests for delete_file, mkdir, send_signal, port_forward ops, auth passthrough
- `tests/test_async_client.py` — mirrors test_client.py but async
- `tests/test_async_sandbox.py` — mirrors test_sandbox.py but async
- `tests/test_tools.py` — update for exception-raising behavior

### P10. Session 1 Commit Plan
1. `feat(python-sdk): add error hierarchy with status code mapping`
2. `feat(python-sdk): add auth support with Bearer token`
3. `feat(python-sdk): add missing methods (deleteFile, mkdir, sendSignal, portForward)`
4. `feat(python-sdk): add client-level methods (listSandboxes, poolStatus, health)`
5. `feat(python-sdk): add ExecSession with WebSocket stdin support`
6. `feat(python-sdk): add AsyncSandbox and AsyncAgentBoxClient`
7. `fix(python-sdk): handle_tool_call raises exceptions instead of error dicts`
8. `feat(python-sdk): update exports in __init__.py`
9. Failing tests for Session 2 (TS SDK) if time permits

---

## Session 2: TypeScript SDK + Daemon WS Auth

### T1. Daemon: Query Param Auth for WS
**Modify:** `crates/agentbox-daemon/src/routes.rs`
- In `require_api_key`: after checking `Authorization` header fails, also check URL query param `token`
- If `?token=<key>` matches expected key, allow the request
- This enables WS connections that can't send custom headers

### T2. Auth Support
**Modify:** `sdks/typescript/src/client.ts`
- Add `apiKey?: string` to constructor
- Resolve from: explicit arg → `process.env.AGENTBOX_API_KEY` → undefined
- If set, add `Authorization: Bearer <key>` header to all fetch requests
- Store `apiKey` for WS URL construction

**Modify:** `sdks/typescript/src/sandbox.ts`
- Add `api_key?: string` (or `apiKey`) to create options
- Pass through to `AgentBoxClient`
- Append `?token=<key>` to WS URL when apiKey is set

### T3. AgentBoxError Class
**New in:** `sdks/typescript/src/errors.ts`
- `AgentBoxError extends Error` with `statusCode: number` and `responseBody: string`
- Replace plain `new Error(...)` in client.ts with `new AgentBoxError(...)`

### T4. Missing Methods
**Modify:** `sdks/typescript/src/sandbox.ts`
- `deleteFile(path: string): Promise<void>` — `DELETE /sandboxes/{id}/files?path=...`
- `mkdir(path: string): Promise<void>` — `PUT /sandboxes/{id}/files?path=...`
- `sendSignal(signal: number): Promise<void>` — `POST /sandboxes/{id}/signal`

**Modify:** `sdks/typescript/src/client.ts`
- `put(path: string, params?: Record<string, string>): Promise<unknown>` — for mkdir

### T5. Static Methods
**Modify:** `sdks/typescript/src/sandbox.ts` (or new utility)
- `Sandbox.list(options?): Promise<SandboxInfo[]>` — `GET /sandboxes`
- `Sandbox.poolStatus(options?): Promise<PoolStatus>` — `GET /pool/status`
- `Sandbox.health(options?): Promise<HealthStatus>` — `GET /health`

These need a temporary client, or accept `url`/`apiKey` directly.

**Modify:** `sdks/typescript/src/types.ts`
- Add `PoolStatus` and `HealthStatus` types

### T6. WS Stdin Support + Error Fix (no breaking change)
**Modify:** `sdks/typescript/src/sandbox.ts`
- Keep `execStream(command)` as-is (returns `AsyncGenerator<ExecStreamEvent>`)
- Add new `execInteractive(command)` returning `ExecSession`:
  - `events(): AsyncGenerator<ExecStreamEvent>` — the event stream
  - `sendStdin(data: Uint8Array): void` — sends `{"type":"stdin","data":"<b64>"}` on active WS
  - `sendSignal(signal: number): void` — sends `{"type":"signal","signal":N}` on active WS
  - `close(): void` — closes the WS
- Fix `ws.onerror` in both `execStream` and `execInteractive`: push an error event to the message queue instead of silently setting `done = true`

**Modify/new:** `sdks/typescript/src/types.ts`
- Add `ExecSession` interface

### T7. Export Fixes
**Modify:** `sdks/typescript/src/index.ts`
- Export `AgentBoxClient` from client.ts
- Export `getToolDefinitions`, `handleToolCall` from tools.ts
- Export `AgentBoxError` from errors.ts
- Export new types: `PoolStatus`, `HealthStatus`, `ExecSession`

### T8. Tool Types
**Modify:** `sdks/typescript/src/tools.ts`
- Add discriminated union for tool call results:
  ```ts
  type ExecToolResult = { stdout: string; stderr: string; exit_code: number }
  type WriteToolResult = { status: "written"; path: string }
  type ReadToolResult = { content: string }
  type ToolError = { error: string }
  type ToolResult = ExecToolResult | WriteToolResult | ReadToolResult | ToolError
  ```
- Type `handleToolCall` return as `Promise<ToolResult>`

### T9. Tests
**Modify/new test files:**
- Update `tests/sdk.test.js` with: auth tests, new methods, error class, WS stdin/error tests
- Or split into multiple files: `tests/client.test.js`, `tests/sandbox.test.js`, `tests/tools.test.js`

### T10. Session 2 Commit Plan
1. `feat(daemon): add query param auth fallback for WebSocket connections`
2. `feat(ts-sdk): add AgentBoxError class and auth support`
3. `feat(ts-sdk): add missing methods (deleteFile, mkdir, sendSignal)`
4. `feat(ts-sdk): add static methods (list, poolStatus, health)`
5. `feat(ts-sdk): add ExecSession with stdin support and fix WS error handling`
6. `feat(ts-sdk): add discriminated union tool types`
7. `feat(ts-sdk): update exports`

---

## Session 3 (if needed): Cross-SDK Polish
- Any overflow from Sessions 1-2
- README updates for both SDKs documenting new features
- Integration test patterns (if daemon is available locally)
