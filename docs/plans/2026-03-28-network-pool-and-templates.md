# Design: Network Warm Pool + Template System

**Date:** 2026-03-28
**Status:** Approved

## Problem

Two performance bottlenecks in AgentBox sandbox creation:

1. **Network sandboxes are always on-demand.** The warm pool only pre-warms VMs without networking (`network: false`). Any `network: true` request falls through to `create_fresh()` — a full Firecracker boot + TAP setup + guest network config, taking seconds instead of milliseconds.

2. **Node.js deps install at runtime.** The rootfs includes `nodejs` and `npm`, but no project dependencies. Every sandbox that needs TypeScript spends ~60s running `npm install` before doing useful work.

## Solution

### Network Warm Pool

Add a second warm queue (`network_available`) to the existing `Pool` struct, controlled by a new `network_min_size` config field. The pool's background loop pre-warms network VMs by:

1. Calling `create_from_snapshot()` with `network: true` (which internally calls `create_fresh()` with TAP setup)
2. Waiting for guest agent readiness
3. Calling `setup_guest_network()` to configure eth0 inside the guest
4. Adding the fully-networked VM to the `network_available` queue

On claim, `network: true` requests pop from `network_available` (instant) before falling back to on-demand. `max_size` remains a global cap across both queues + active sandboxes.

**Config:**
```toml
[pool]
min_size = 2             # non-network warm VMs
max_size = 10            # global cap (all VMs)
network_min_size = 1     # network-enabled warm VMs
idle_timeout_secs = 3600
```

**Default:** `network_min_size = 0` — backward compatible, no network VMs pre-warmed unless configured.

### Template Build System

A build-time template mechanism that produces per-template rootfs images with pre-installed dependencies.

**Structure:**
```
artifacts/templates/
├── build-template.sh    # Generic builder
└── node/
    ├── package.json     # Dependencies to pre-install
    └── setup.sh         # Runs inside chroot
```

**Build flow:**
1. `build-template.sh` copies base `rootfs.ext4` → `rootfs-<name>.ext4` (CoW)
2. Mounts the copy, copies template files into `/workspace/`
3. Runs `setup.sh` inside chroot (e.g., `npm install`)
4. Unmounts, producing a self-contained rootfs

**Snapshot:** Each template gets its own snapshot via `bake.sh --rootfs <name>`, stored in `snapshot-<name>/`.

**Default node template:** typescript, tsx, @types/node — minimal starter. Users add project-specific deps at runtime.

## Alternatives Considered

### Pool Design
- **Separate Pool instances:** More isolation but doubles config complexity and resource overhead. Single pool with ratio config is simpler.
- **Network setup at claim time:** Pre-warm only boots VM, network configures on claim. Saves idle TAP devices but adds latency to claims — defeats the purpose of warm pool.

### Template System
- **Rootfs overlay only:** Single template baked into rootfs. No multi-template support.
- **Runtime API + snapshot:** SDK-driven template creation. Most flexible but requires new API endpoints, template registry, and pool-per-template selection. Future work.

## Non-Goals
- Runtime template creation API (future)
- Pool-per-template selection (pool currently uses a single rootfs; template-aware pool is follow-up)
- Network snapshots (Firecracker doesn't support snapshotting with TAP; network VMs always fresh-boot)
