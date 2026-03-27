# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

AgentBox is a self-hosted sandbox infrastructure for AI agents — a Rust daemon + SDK that gives any AI agent a sandboxed Firecracker microVM to run code. Like E2B, but self-hosted with a one-command install.

## Build & Development Commands

```bash
# Build
cargo build                              # All crates (debug)
cargo build --release                    # All crates (release)
cargo build -p agentbox-daemon           # Single crate
cargo build -p guest-agent --target x86_64-unknown-linux-gnu  # Cross-compile guest

# Test
cargo test                               # All tests
cargo test -p agentbox-core              # Single crate
cargo test -p guest-agent -- test_name   # Single test

# Lint & Format
cargo clippy --all-targets               # Lint
cargo fmt                                # Format
cargo fmt --check                        # Check formatting

# Run
cargo run -p agentbox-daemon             # Start daemon (reads /var/lib/agentbox/config.toml)
cargo run -p agentbox-daemon -- ./config.example.toml  # Custom config
cargo run -p guest-agent -- --port 5000 --tcp           # Guest agent in TCP mode (local dev)

# VM Artifacts (requires Linux with KVM)
cd artifacts && make all                 # Build kernel + rootfs + guest-agent + snapshot
```

## Architecture

Cargo workspace with 4 crates + 2 SDKs. Library-first design: all VM logic lives in `agentbox-core`, daemon and CLI are thin wrappers.

```
SDK (Python/TS) → HTTP/WS → agentbox-daemon (axum) → agentbox-core (library)
                                                          ├── Sandbox (user-facing API)
                                                          ├── Pool (warm VM management)
                                                          ├── VmManager (Firecracker lifecycle)
                                                          └── VsockClient (host↔guest protocol)
                                                                    ↓ vsock
                                                              guest-agent (inside microVM)
                                                                ├── Exec (PTY + pipes)
                                                                ├── Files (read/write/list)
                                                                └── Protocol (length-prefixed JSON)
```

### Crate Responsibilities

- **agentbox-core** (`crates/agentbox-core/`): Library with all VM management types — Sandbox, Pool, VmManager, VsockClient, Config, Error. VsockClient and Config are complete; Sandbox/Pool/VmManager are stubs (`todo!()`).
- **agentbox-daemon** (`crates/agentbox-daemon/`): axum HTTP/WS server. REST API for sandbox CRUD + exec + files. WebSocket handler for streaming exec. Uses `AppState` with `Arc<Pool>` + sandbox registry.
- **guest-agent** (`crates/guest-agent/`): Runs inside the microVM. Listens on vsock (or TCP for dev). Handles exec (with PTY support), file operations, and signals. Uses length-prefixed JSON protocol.
- **agentbox-cli** (`crates/agentbox-cli/`): CLI binary — currently a stub.

### Communication Protocols

- **Host ↔ Guest (vsock)**: 4-byte big-endian length prefix + JSON payload. Request/response multiplexed via monotonic `id` field. Streaming exec sends multiple `stream` frames followed by a final `result` frame.
- **Client ↔ Daemon (HTTP)**: Standard REST. Binary data (files, stdin/stdout) is base64-encoded in JSON.
- **Client ↔ Daemon (WebSocket)**: JSON messages with `type` field (`exec`, `stdin`, `signal` from client; `ready`, `stdout`, `stderr`, `exit`, `error` from server). Uses `tokio::select!` for concurrent reads.

### Error Handling Pattern

Two-layer errors: `AgentBoxError` (thiserror enum in core) maps to `AppError` (HTTP status codes in daemon) via `From` impl. Custom `Result<T>` type alias in core.

### Concurrency Patterns

- `Arc<Mutex<>>` for shared sandbox state
- `Arc<RwLock<HashMap>>` for sandbox registry (multiple readers)
- `tokio::select!` for concurrent WS event handling
- `mpsc` channels for streaming output

## SDKs

- **Python** (`sdks/python/`): Uses `httpx` + `websockets` + `pydantic`. Install with `uv pip install -e sdks/python`.
- **TypeScript** (`sdks/typescript/`): Uses native `fetch`.

## Key Config

See `config.example.toml` for all daemon options. Config is parsed by `AgentBoxConfig::from_file()` with serde defaults for all sections.
