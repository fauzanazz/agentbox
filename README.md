# AgentBox

Self-hosted sandbox infrastructure for AI agents. Boot isolated Firecracker microVMs in <300ms via mmap snapshot restore and safely execute arbitrary code.

> **Open-source alternative to E2B** — own your infrastructure, zero vendor lock-in.

## Features

- **Fast boot** — mmap snapshot restore gets a sandbox ready in <300ms
- **Strong isolation** — each sandbox is a separate Firecracker microVM with its own kernel and filesystem
- **Streaming execution** — real-time stdout/stderr over WebSocket
- **File operations** — upload, download, and list files inside sandboxes
- **Warm VM pool** — pre-booted VMs for instant allocation
- **LLM tool integration** — built-in tool definitions for OpenAI and Anthropic formats
- **Python & TypeScript SDKs** — thin, ergonomic client libraries with zero vendor lock-in

## Architecture

```
┌──────────────────────────────────────┐
│  Your AI App (Python/TS agent loop)  │
└──────────────┬───────────────────────┘
               │ HTTP + WebSocket
               ▼
┌──────────────────────────────────────┐
│  agentbox-daemon  (Axum server)      │
│  └─ agentbox-core (VM pool + vsock)  │
└──────────────┬───────────────────────┘
               │ Firecracker API + vsock
               ▼
┌──────────────────────────────────────┐
│  Firecracker microVM                 │
│  └─ guest-agent (Rust binary)        │
│  Alpine Linux · Python 3.12 · Node 22│
└──────────────────────────────────────┘
```

## Quickstart

### Install

```bash
curl -fsSL https://raw.githubusercontent.com/fauzanazz/agentbox/main/scripts/install.sh | sh
```

This downloads pre-built binaries and VM artifacts, bakes a snapshot, and starts the daemon via systemd.

**Build from source** (if no release for your platform):

```bash
curl -fsSL https://raw.githubusercontent.com/fauzanazz/agentbox/main/scripts/setup.sh | sh
```

### Python SDK

```bash
pip install agentbox
```

```python
from agentbox import Sandbox

with Sandbox.create() as sb:
    result = sb.exec("echo hello world")
    print(result.stdout)  # "hello world\n"
```

### TypeScript SDK

```bash
npm install agentbox
```

```typescript
import { Sandbox } from "agentbox";

const sb = await Sandbox.create();
const result = await sb.exec("echo hello world");
console.log(result.stdout); // "hello world\n"
await sb.destroy();
```

### Streaming Execution

```python
import asyncio
from agentbox import Sandbox

async def main():
    sb = Sandbox.create()
    async for event in sb.exec_stream("python -c 'print(1+1)'"):
        if event["type"] == "stdout":
            print(event["data"], end="")
        elif event["type"] == "exit":
            print(f"Exit code: {event['code']}")
    sb.destroy()

asyncio.run(main())
```

```typescript
import { Sandbox } from "agentbox";

const sb = await Sandbox.create();
for await (const event of sb.execStream("python -c 'print(1+1)'")) {
  if (event.type === "stdout") process.stdout.write(event.data!);
  else if (event.type === "exit") console.log(`Exit code: ${event.code}`);
}
await sb.destroy();
```

### LLM Tool Integration

```python
from agentbox import Sandbox

with Sandbox.create() as sb:
    # Get tool definitions for your LLM provider
    tools = sb.tool_definitions(format="openai")   # or "anthropic"

    # After LLM returns a tool call, execute it
    result = sb.handle_tool_call(tool_call)
```

```typescript
import { Sandbox } from "agentbox";

const sb = await Sandbox.create();
const tools = sb.toolDefinitions("openai"); // or "anthropic"

// After LLM returns a tool call, execute it
const result = await sb.handleToolCall(toolCall);
await sb.destroy();
```

## CLI

```
agentbox serve [--config PATH] [--listen ADDR]   Start the daemon
agentbox --version                                Show version
```

## HTTP API

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/sandboxes` | Create a sandbox |
| `GET` | `/sandboxes` | List active sandboxes |
| `GET` | `/sandboxes/{id}` | Get sandbox info |
| `POST` | `/sandboxes/{id}/exec` | Execute command |
| `GET` | `/sandboxes/{id}/ws` | WebSocket streaming exec |
| `POST` | `/sandboxes/{id}/files` | Upload file (multipart) |
| `GET` | `/sandboxes/{id}/files?path=...` | Download / list files |
| `DELETE` | `/sandboxes/{id}` | Destroy sandbox |
| `GET` | `/health` | Health check + pool stats |

See [docs/api.md](docs/api.md) for full API reference with request/response examples.

## Configuration

Copy `config.example.toml` and adjust as needed:

```toml
[daemon]
listen = "127.0.0.1:8080"
log_level = "info"

[vm]
firecracker_bin = "/usr/local/bin/firecracker"
kernel_path = "/var/lib/agentbox/vmlinux"
rootfs_path = "/var/lib/agentbox/rootfs.ext4"
snapshot_path = "/var/lib/agentbox/snapshot"

[vm.defaults]
memory_mb = 512
vcpus = 1
network = false
timeout_secs = 3600

[pool]
min_size = 2
max_size = 10
idle_timeout_secs = 3600
```

## Project Structure

```
crates/
├── agentbox-core/      # Core library — VM lifecycle, pool, vsock, config
├── agentbox-daemon/    # HTTP/WebSocket server (Axum)
├── agentbox-cli/       # CLI management tool
└── guest-agent/        # Binary that runs inside each microVM
sdks/
├── python/             # Python SDK (httpx + pydantic + websockets)
└── typescript/         # TypeScript SDK (zero deps, native fetch)
artifacts/              # VM artifact build scripts (kernel, rootfs, snapshot)
scripts/                # Install + setup scripts, systemd service
```

## Development

### Prerequisites

- Rust (2021 edition)
- Linux with KVM support (Firecracker requirement) — macOS works for building, Linux needed for running

### Build

```bash
cargo build --workspace
```

### Test

```bash
# Rust crates
cargo test --workspace

# TypeScript SDK
cd sdks/typescript && pnpm install && pnpm test

# Python SDK
cd sdks/python && pip install -e ".[dev]" && pytest
```

## Tech Stack

- **Runtime**: Rust + Tokio
- **HTTP**: Axum 0.8
- **Virtualization**: Firecracker microVMs
- **Guest OS**: Alpine Linux 3.20 with Python 3.12 and Node.js 22
- **Communication**: vsock with length-prefixed JSON protocol
- **Python SDK**: httpx, pydantic, websockets
- **TypeScript SDK**: zero dependencies (native fetch + WebSocket)

## Requirements

- Linux with KVM (`/dev/kvm`) — Firecracker does not support macOS/Windows natively
- x86_64 or aarch64 architecture

## Supported Environments

AgentBox uses Firecracker microVMs which require Linux with KVM. The kernel and environment requirements differ between bare-metal and nested virtualization.

### Bare-metal (recommended)

Any Linux host with KVM support. Works with the default kernel 4.14 config or newer kernels (5.10, 6.1).

Known working:
- AWS bare-metal instances (`.metal`)
- Dedicated servers (Hetzner, OVH, etc.)
- Local Linux machines with KVM enabled

### Nested virtualization (cloud VMs)

Most cloud VMs run inside a hypervisor, creating a nested KVM environment. Kernels 5.10+ fail with `-EINVAL` on virtio device probe due to strict DMA/IOMMU feature negotiation that doesn't work under nested KVM.

**AgentBox ships kernel 4.14.336 by default**, which works in both bare-metal and nested environments.

Known working:
- DigitalOcean droplets (with KVM enabled)
- AWS non-bare-metal EC2 instances (with nested virtualization enabled)
- GCP instances with nested virtualization

If you see errors like `virtio_blk: probe of virtio0 failed with error -22` or `vmw_vsock_virtio_transport: probe of virtio1 failed with error -22`, you're hitting the nested virt kernel issue. Ensure you're using the shipped 4.14 kernel, not a custom 5.10+ build.

### Building artifacts from source

If pre-built artifacts aren't available for your architecture, build them locally:

```bash
cd artifacts && make all    # Requires: Linux with KVM, build-essential, curl, socat
```

This builds the kernel (~10 min), rootfs, and snapshot. The rootfs build auto-downloads `apk-tools-static` if the Alpine package manager isn't available on your host.

## License

[Apache-2.0](LICENSE)
