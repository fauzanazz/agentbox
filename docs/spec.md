# Spec: AgentBox — Self-Hosted Sandbox Infrastructure for AI Agents

## 1. One-Line Summary

A Rust daemon + SDK that gives any AI agent a sandboxed computer (Firecracker microVM)
to run bash, Python, and arbitrary code — booting in <300ms via mmap snapshot restore.
Like E2B, but self-hosted and setup is one `curl | sh`.

## 2. The Problem

Developers building AI chat/agent products (customer support bots, AI data analysts,
AI coding assistants, AI tutors) face a critical gap: **their AI can talk, but it
can't *do*.**

Most agentic AI chats today are "generic" — they can answer questions and generate
text, but they can't run code, install packages, process files, or use tools that
require a shell. The dynamic nature of user requests means the AI needs freedom to
run arbitrary bash/Python — you can't pre-define every tool.

Setting up safe code execution is hard:
- Docker is slow to spin up and has escape vectors
- Running on the host is a security nightmare
- E2B works but it's a paid cloud service, vendor lock-in, AND hard to self-host
  (bad docs, many manual steps)
- Building your own sandbox infra is months of work

**AgentBox solves this:** `curl | sh` and your machine is ready. A simple SDK to
create sandboxes, execute code, and transfer files. Firecracker microVMs for real
isolation at near-instant boot times.

### Core Differentiator vs E2B

E2B's self-hosted setup requires multiple manual steps, separate infrastructure
components, and poorly documented configuration. **AgentBox's entire setup is:**

```bash
curl -fsSL https://agentbox.dev/install.sh | sh
```

That single script:
1. Detects architecture (x86_64 / aarch64)
2. Verifies KVM access (exits with clear error if unavailable)
3. Downloads pre-built agentbox binary
4. Downloads pre-built Firecracker binary
5. Downloads pre-built artifacts (kernel, rootfs, base snapshot)
6. Installs systemd service (or prints manual start command)
7. Starts the daemon

**Zero compilation. Zero manual steps. Machine is ready in under 60 seconds.**

## 3. Target Users

Developers building AI-powered products who need to give their AI code execution:

- **AI Chat Products** — chatbots that can run code to answer questions, analyze data,
  generate charts, process uploads
- **AI Coding Tools** — IDE extensions or web apps where AI writes and runs code
- **AI Data Analysts** — agents that install packages, run pandas/SQL, produce reports
- **AI Tutors** — educational platforms where AI executes student code safely
- **Internal Tools** — company AI assistants that can run scripts, query databases,
  automate workflows

**Primary persona:** A backend developer building an AI chat product with any LLM
(Claude, GPT, Gemini, local models). They want to add "run code" capabilities without
spending weeks on sandbox infrastructure.

## 4. Success Criteria

### Zero-Friction Setup
1. `curl -fsSL https://agentbox.dev/install.sh | sh` — downloads pre-built binary +
   pre-built artifacts (kernel, rootfs, snapshot), installs systemd service, starts
   daemon. No compilation. No manual steps.
2. Pre-built release artifacts hosted on GitHub Releases:
   - `agentbox-linux-x86_64` / `agentbox-linux-aarch64` (static binary)
   - `agentbox-artifacts-x86_64.tar.gz` (kernel + rootfs + snapshot + guest-agent)
3. Install script is idempotent — running again updates if newer version available.
4. Clear error messages: "KVM not available — run on a bare-metal machine or enable
   nested virtualization."

### Simple SDK Integration
5. Python SDK: `pip install agentbox` (or `uv add agentbox`)
6. TypeScript SDK: `npm install agentbox` (or `pnpm add agentbox`)
7. Core operations:

```python
from agentbox import Sandbox

# Create a sandbox (boots Firecracker VM in <300ms)
sandbox = Sandbox.create()

# Execute commands
result = sandbox.exec("pip install pandas matplotlib")
result = sandbox.exec("python analyze.py")
print(result.stdout, result.stderr, result.exit_code)

# File operations
sandbox.upload("data.csv", "/workspace/data.csv")
content = sandbox.download("/workspace/output.png")
files = sandbox.list_files("/workspace")

# Cleanup
sandbox.destroy()
```

8. The SDK is a thin HTTP client — all logic lives in the Rust daemon.
9. SDK auto-discovers daemon URL (localhost:8080 default, configurable via
   `AGENTBOX_URL` env var or constructor param).

### Pre-Built Tool Definitions (convenience layer)
10. Ship ready-made tool schemas for popular LLM frameworks so developers can plug
    sandbox execution directly into their agent's tool-calling loop:

```python
# OpenAI function calling
tools = sandbox.tool_definitions("openai")
# Returns: [{"type": "function", "function": {"name": "execute_code", ...}}]

# Anthropic tool_use
tools = sandbox.tool_definitions("anthropic")
# Returns: [{"name": "execute_code", "input_schema": {...}}]

# Generic (framework-agnostic)
tools = sandbox.tool_definitions("generic")
```

11. Tool execution handler that maps LLM tool calls to sandbox operations:

```python
# In your agent loop:
for tool_call in llm_response.tool_calls:
    result = sandbox.handle_tool_call(tool_call)
    # Automatically routes execute_code, read_file, write_file, etc.
```

### Fast Boot via mmap Snapshots
12. Firecracker snapshot restore with mmap-backed guest memory — VM resumes from a
    frozen state instead of cold booting.
13. Target: <300ms from `Sandbox.create()` to sandbox ready for commands.
14. Base snapshot includes: Alpine Linux, Python 3.12, Node.js 22, git, ripgrep, jq,
    curl, build-essential, and the guest agent daemon already running.

### Strong Isolation
15. Each sandbox is its own microVM — separate kernel, filesystem, network namespace.
16. No shared filesystem with host (files transferred via vsock guest agent).
17. Network: disabled by default, opt-in per sandbox (`Sandbox.create(network=True)`).
18. Resource limits: configurable memory (default 2GB) and vCPUs (default 2).

### VM Pool (warm sandboxes)
19. The daemon maintains a configurable pool of pre-booted VMs.
20. `Sandbox.create()` claims a warm VM from the pool instead of cold-starting.
21. Pool replenishes in the background as VMs are claimed.
22. Configurable: min pool size, max pool size, idle timeout.

### Guest Agent (vsock daemon inside VM)
23. Small Rust binary inside the VM, communicating with host daemon via vsock.
24. Capabilities: execute commands (with streaming stdout/stderr), file read/write,
    file listing, process management, signal forwarding.
25. Length-prefixed JSON protocol over vsock (proven in claude-harness).

### Host Daemon (Rust)
26. Long-running process managing VM lifecycle, pool, and HTTP API.
27. HTTP API endpoints:
    - `POST /sandboxes` — create sandbox (claim from pool)
    - `POST /sandboxes/{id}/exec` — execute command (streaming response)
    - `POST /sandboxes/{id}/files` — upload file
    - `GET /sandboxes/{id}/files/{path}` — download file
    - `GET /sandboxes/{id}/files` — list files
    - `DELETE /sandboxes/{id}` — destroy sandbox
    - `GET /sandboxes` — list active sandboxes
    - `GET /health` — health check + pool status
28. Deployment: sidecar on same machine or separate infra server. SDK accepts
    configurable URL (defaults to `http://localhost:8080`).

### CLI (management + debugging)
29. All management is via the same `agentbox` binary:
    - `agentbox serve` — start daemon (foreground, for debugging)
    - `agentbox list` — show active sandboxes
    - `agentbox exec <sandbox-id> "command"` — execute a command (for debugging)
    - `agentbox stop <sandbox-id>` — destroy a sandbox
    - `agentbox logs <sandbox-id>` — tail sandbox logs
    - `agentbox status` — daemon health, pool stats, KVM info

## 5. Out of Scope (MVP)

- **Web UI / dashboard** — CLI, HTTP API, and SDKs only
- **Multi-node / clustering** — single machine only, no distributed scheduling
- **Agent orchestration** — no multi-agent coordination, task routing, or DAGs;
  each sandbox is independent
- **LLM routing** — AgentBox doesn't call LLMs; developers own their agent loop
- **macOS / Windows native** — KVM is Linux-only; macOS developers can use Lima
  (documented but not automated)
- **Persistent sandboxes** — VMs are ephemeral; no resume after destroy
- **Authentication / multi-tenancy** — single-user, no auth on the API
- **Custom rootfs images** — MVP ships one rootfs; custom images come later
- **GPU passthrough** — CPU-only sandboxes for MVP

## 6. Constraints

- **Host daemon:** Rust, single static binary, `tokio` async runtime
- **HTTP framework:** `axum`
- **CLI framework:** `clap`
- **Isolation:** Firecracker microVMs (Linux + KVM required)
- **Memory:** mmap-backed guest memory for instant snapshot restore
- **Guest agent:** Rust binary, vsock, length-prefixed JSON protocol
- **Python SDK:** minimal dependencies, `httpx` for HTTP, `pydantic` for types
- **TypeScript SDK:** zero/minimal dependencies, native `fetch`
- **Config:** TOML for daemon configuration
- **Distribution:** Pre-built binaries + artifacts on GitHub Releases. `curl | sh`
  installer. No compilation required for users.
- **License:** Apache-2.0
- **Guest OS:** Alpine Linux (small footprint, fast rootfs build)
- **Build pipeline:** GitHub Actions CI builds binaries + artifacts for releases
