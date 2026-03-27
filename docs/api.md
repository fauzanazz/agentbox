# AgentBox HTTP API Reference

Base URL: `http://localhost:8080` (configurable via `daemon.listen` in config)

---

## Create Sandbox

**POST** `/sandboxes`

Boot a new microVM sandbox.

**Request body:**
```json
{
  "memory_mb": 2048,
  "vcpus": 2,
  "network": false,
  "timeout": 3600
}
```

All fields are optional — defaults are used from server config.

**Response:** `201 Created`
```json
{
  "id": "a1b2c3d4e5f6",
  "status": "Ready",
  "config": {
    "memory_mb": 2048,
    "vcpus": 2,
    "network": false,
    "timeout_secs": 3600
  },
  "created_at": "0s ago"
}
```

**Errors:**
- `503 Service Unavailable` — pool exhausted, no VMs available

---

## List Sandboxes

**GET** `/sandboxes`

Returns all active sandboxes.

**Response:** `200 OK`
```json
[
  {
    "id": "a1b2c3d4e5f6",
    "status": "Ready",
    "config": { "memory_mb": 2048, "vcpus": 2, "network": false, "timeout_secs": 3600 },
    "created_at": "42s ago"
  }
]
```

---

## Get Sandbox

**GET** `/sandboxes/{id}`

**Response:** `200 OK` — same shape as create response.

**Errors:**
- `404 Not Found` — sandbox does not exist

---

## Execute Command

**POST** `/sandboxes/{id}/exec`

Execute a command and wait for completion.

**Request body:**
```json
{
  "command": "echo hello && python3 -c 'print(2+2)'",
  "timeout": 30
}
```

`timeout` is optional (default: 30 seconds).

**Response:** `200 OK`
```json
{
  "stdout": "hello\n4\n",
  "stderr": "",
  "exit_code": 0
}
```

**Errors:**
- `404 Not Found` — sandbox does not exist

---

## Streaming Execution (WebSocket)

**GET** `/sandboxes/{id}/ws`

Upgrade to WebSocket for real-time streaming execution.

### Protocol

1. Server sends `{"type": "ready"}` on connection
2. Client sends exec command:
   ```json
   {"type": "exec", "command": "python3 script.py", "timeout": 30}
   ```
3. Server streams events:
   ```json
   {"type": "stdout", "data": "<base64-encoded>"}
   {"type": "stderr", "data": "<base64-encoded>"}
   {"type": "exit", "code": 0}
   ```
4. Client can send stdin:
   ```json
   {"type": "stdin", "data": "<base64-encoded>"}
   ```
5. Client can send signals:
   ```json
   {"type": "signal", "signal": 9}
   ```

**Error event:**
```json
{"type": "error", "message": "description of error"}
```

---

## Upload File

**POST** `/sandboxes/{id}/files`

Upload a file via multipart form data.

**Request:** `Content-Type: multipart/form-data`
- `file` — file content (required)
- `path` — destination path in sandbox (optional, default: `/workspace/upload`)

**Example:**
```bash
curl -X POST http://localhost:8080/sandboxes/{id}/files \
  -F "path=/workspace/script.py" \
  -F "file=@./script.py"
```

**Response:** `200 OK`
```json
{
  "path": "/workspace/script.py",
  "size": 1234
}
```

---

## Download File

**GET** `/sandboxes/{id}/files?path=/workspace/output.txt`

Download a file from the sandbox.

**Response:** `200 OK` with `Content-Type: application/octet-stream` and raw file bytes.

---

## List Files

**GET** `/sandboxes/{id}/files?list=true&path=/workspace`

List files in a directory.

**Response:** `200 OK`
```json
[
  { "name": "script.py", "size": 1234, "is_dir": false },
  { "name": "output", "size": 0, "is_dir": true }
]
```

---

## Destroy Sandbox

**DELETE** `/sandboxes/{id}`

Destroy the sandbox and its VM.

**Response:** `200 OK`
```json
{
  "status": "destroyed"
}
```

**Errors:**
- `404 Not Found` — sandbox does not exist
- `400 Bad Request` — sandbox is currently in use by another request

---

## Health Check

**GET** `/health`

**Response:** `200 OK`
```json
{
  "status": "ok",
  "pool": {
    "active": 3,
    "max_size": 10
  }
}
```

---

## Error Format

All errors return JSON:

```json
{
  "error": "Human-readable error description"
}
```

| Status | Meaning |
|--------|---------|
| `400` | Bad request — invalid input or sandbox in use |
| `404` | Not found — sandbox does not exist |
| `500` | Internal server error |
| `503` | Service unavailable — pool exhausted |
