# Build Pipeline (Makefile)

## Context

The Makefile builds all artifacts needed for Firecracker VMs: kernel, rootfs,
guest agent binary, and snapshot. These are built in CI and published as
release tarballs on GitHub Releases.

This task assumes the guest-agent crate exists from FAU-68.
See `docs/architecture.md` for the artifact structure.

## Requirements

- Build minimal Linux kernel (vmlinux) for Firecracker
- Build Alpine rootfs with Python, Node.js, and dev tools
- Cross-compile guest-agent for the target architecture
- Bake base snapshot (boot VM, wait for agent, take snapshot)
- Support x86_64 and aarch64
- Output all artifacts to `artifacts/output/{arch}/`

## Implementation

### `artifacts/Makefile`

```makefile
ARCH ?= $(shell uname -m)
OUTPUT_DIR := output/$(ARCH)
KERNEL_VERSION := 6.1.102
ALPINE_VERSION := 3.20
ROOTFS_SIZE_MB := 512

.PHONY: all kernel rootfs guest-agent snapshot clean

all: kernel rootfs guest-agent snapshot

$(OUTPUT_DIR):
	mkdir -p $(OUTPUT_DIR)

# === Kernel ===
kernel: $(OUTPUT_DIR)/vmlinux

$(OUTPUT_DIR)/vmlinux: $(OUTPUT_DIR)
	./kernel/build.sh $(KERNEL_VERSION) $(ARCH) $(OUTPUT_DIR)

# === Rootfs ===
rootfs: $(OUTPUT_DIR)/rootfs.ext4

$(OUTPUT_DIR)/rootfs.ext4: guest-agent $(OUTPUT_DIR)
	./rootfs/build.sh $(ALPINE_VERSION) $(ARCH) $(OUTPUT_DIR) $(ROOTFS_SIZE_MB)

# === Guest Agent ===
guest-agent: $(OUTPUT_DIR)/guest-agent

$(OUTPUT_DIR)/guest-agent: $(OUTPUT_DIR)
	./guest-agent/build.sh $(ARCH) $(OUTPUT_DIR)

# === Snapshot ===
snapshot: $(OUTPUT_DIR)/snapshot/vmstate.bin

$(OUTPUT_DIR)/snapshot/vmstate.bin: kernel rootfs $(OUTPUT_DIR)
	./snapshot/bake.sh $(ARCH) $(OUTPUT_DIR)

# === Clean ===
clean:
	rm -rf output/
```

### `artifacts/kernel/build.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail

KERNEL_VERSION="$1"
ARCH="$2"
OUTPUT_DIR="$3"

MAJOR_VERSION=$(echo "$KERNEL_VERSION" | cut -d. -f1-2)
SRC_DIR="build/linux-${KERNEL_VERSION}"
VMLINUX="${OUTPUT_DIR}/vmlinux"

if [ -f "$VMLINUX" ]; then
    echo "Kernel already built: $VMLINUX"
    exit 0
fi

echo "Building Linux kernel ${KERNEL_VERSION} for ${ARCH}..."

# Download kernel source
mkdir -p build
if [ ! -d "$SRC_DIR" ]; then
    curl -fsSL "https://cdn.kernel.org/pub/linux/kernel/v${MAJOR_VERSION%%.*}.x/linux-${KERNEL_VERSION}.tar.xz" \
        | tar xJ -C build/
fi

# Apply Firecracker-minimal config
KCONFIG="kernel/config-${ARCH}"
if [ ! -f "$KCONFIG" ]; then
    echo "ERROR: No kernel config for ${ARCH}"
    exit 1
fi
cp "$KCONFIG" "${SRC_DIR}/.config"

# Build
make -C "$SRC_DIR" -j"$(nproc)" vmlinux

# Copy output
cp "${SRC_DIR}/vmlinux" "$VMLINUX"
echo "Kernel built: $VMLINUX"
```

### `artifacts/kernel/config-x86_64`

Minimal Firecracker kernel config. Key options:
```
# Base
CONFIG_64BIT=y
CONFIG_SMP=y
CONFIG_PRINTK=y

# Virtio (required by Firecracker)
CONFIG_VIRTIO=y
CONFIG_VIRTIO_PCI=y
CONFIG_VIRTIO_MMIO=y
CONFIG_VIRTIO_BLK=y
CONFIG_VIRTIO_NET=y
CONFIG_VIRTIO_CONSOLE=y
CONFIG_VHOST_VSOCK=y
CONFIG_VIRTIO_VSOCK=y

# Filesystem
CONFIG_EXT4_FS=y
CONFIG_TMPFS=y
CONFIG_PROC_FS=y
CONFIG_SYSFS=y
CONFIG_DEVTMPFS=y
CONFIG_DEVTMPFS_MOUNT=y

# Serial console
CONFIG_SERIAL_8250=y
CONFIG_SERIAL_8250_CONSOLE=y

# Networking (minimal)
CONFIG_NET=y
CONFIG_INET=y
CONFIG_IPV6=y

# No modules (everything built-in)
# CONFIG_MODULES is not set

# Disable unnecessary features
# CONFIG_WIRELESS is not set
# CONFIG_SOUND is not set
# CONFIG_USB is not set
# CONFIG_DRM is not set
# CONFIG_FB is not set
```

Create a proper `.config` file by running `make tinyconfig` and adding the
above options on top. Use the provided `config-x86_64` as the full config.

For aarch64, create `config-aarch64` with equivalent options adjusted for ARM.

### `artifacts/rootfs/build.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail

ALPINE_VERSION="$1"
ARCH="$2"
OUTPUT_DIR="$3"
SIZE_MB="$4"

ROOTFS="${OUTPUT_DIR}/rootfs.ext4"
GUEST_AGENT="${OUTPUT_DIR}/guest-agent"
MOUNT_DIR="build/rootfs-mount"

if [ -f "$ROOTFS" ]; then
    echo "Rootfs already built: $ROOTFS"
    exit 0
fi

echo "Building Alpine ${ALPINE_VERSION} rootfs for ${ARCH} (${SIZE_MB}MB)..."

# Map arch names
case "$ARCH" in
    x86_64)  ALPINE_ARCH="x86_64" ;;
    aarch64) ALPINE_ARCH="aarch64" ;;
    *) echo "Unsupported: $ARCH"; exit 1 ;;
esac

# Create ext4 image
dd if=/dev/zero of="$ROOTFS" bs=1M count="$SIZE_MB"
mkfs.ext4 -F "$ROOTFS"

# Mount
mkdir -p "$MOUNT_DIR"
sudo mount -o loop "$ROOTFS" "$MOUNT_DIR"

# Bootstrap Alpine
MIRROR="https://dl-cdn.alpinelinux.org/alpine/v${ALPINE_VERSION}"
sudo apk -X "${MIRROR}/main" -U --allow-untrusted --root "$MOUNT_DIR" \
    --initdb add alpine-base

# Configure repos
sudo mkdir -p "${MOUNT_DIR}/etc/apk"
echo "${MIRROR}/main" | sudo tee "${MOUNT_DIR}/etc/apk/repositories"
echo "${MIRROR}/community" | sudo tee -a "${MOUNT_DIR}/etc/apk/repositories"

# Install packages
sudo chroot "$MOUNT_DIR" apk update
sudo chroot "$MOUNT_DIR" apk add \
    python3 py3-pip nodejs npm \
    git ripgrep jq curl wget \
    build-base gcc musl-dev \
    bash openssh-client

# Copy guest agent
sudo cp "$GUEST_AGENT" "${MOUNT_DIR}/usr/local/bin/guest-agent"
sudo chmod +x "${MOUNT_DIR}/usr/local/bin/guest-agent"

# Copy overlay files (init scripts, etc.)
sudo cp -r overlay/* "${MOUNT_DIR}/"

# Create workspace directory
sudo mkdir -p "${MOUNT_DIR}/workspace"

# Setup OpenRC to start guest-agent on boot
sudo chroot "$MOUNT_DIR" rc-update add guest-agent default

# Set hostname
echo "agentbox" | sudo tee "${MOUNT_DIR}/etc/hostname"

# Enable serial console for Firecracker
echo "ttyS0::respawn:/sbin/getty -L ttyS0 115200 vt100" | \
    sudo tee -a "${MOUNT_DIR}/etc/inittab"

# Cleanup
sudo umount "$MOUNT_DIR"
rmdir "$MOUNT_DIR"

echo "Rootfs built: $ROOTFS"
```

### `artifacts/rootfs/overlay/etc/init.d/guest-agent`

```bash
#!/sbin/openrc-run

description="AgentBox Guest Agent"
command="/usr/local/bin/guest-agent"
command_background=true
pidfile="/run/guest-agent.pid"
output_log="/var/log/guest-agent.log"
error_log="/var/log/guest-agent.log"

depend() {
    after localmount
}
```

### `artifacts/guest-agent/build.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail

ARCH="$1"
OUTPUT_DIR="$2"

BINARY="${OUTPUT_DIR}/guest-agent"

if [ -f "$BINARY" ]; then
    echo "Guest agent already built: $BINARY"
    exit 0
fi

echo "Building guest-agent for ${ARCH}..."

# Map to Rust target
case "$ARCH" in
    x86_64)  TARGET="x86_64-unknown-linux-musl" ;;
    aarch64) TARGET="aarch64-unknown-linux-musl" ;;
    *) echo "Unsupported: $ARCH"; exit 1 ;;
esac

# Build (static binary with musl)
cd ../../crates/guest-agent
cargo build --release --target "$TARGET"
cp "../../target/${TARGET}/release/guest-agent" "$BINARY"

echo "Guest agent built: $BINARY"
```

### `artifacts/snapshot/bake.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail

ARCH="$1"
OUTPUT_DIR="$2"

SNAPSHOT_DIR="${OUTPUT_DIR}/snapshot"
VMLINUX="${OUTPUT_DIR}/vmlinux"
ROOTFS="${OUTPUT_DIR}/rootfs.ext4"
WORK_DIR="build/snapshot-bake"

if [ -f "${SNAPSHOT_DIR}/vmstate.bin" ]; then
    echo "Snapshot already baked: ${SNAPSHOT_DIR}"
    exit 0
fi

echo "Baking base snapshot..."

mkdir -p "$SNAPSHOT_DIR" "$WORK_DIR"

# Copy rootfs (don't modify the original)
cp "$ROOTFS" "${WORK_DIR}/rootfs.ext4"

API_SOCK="${WORK_DIR}/api.sock"
VSOCK_SOCK="${WORK_DIR}/vsock.sock"

# Start Firecracker (fresh boot)
firecracker --api-sock "${API_SOCK}" &
FC_PID=$!
sleep 1

# Configure VM via API
curl --unix-socket "$API_SOCK" -s -X PUT "http://localhost/boot-source" \
    -H "Content-Type: application/json" \
    -d "{\"kernel_image_path\": \"$(realpath "$VMLINUX")\", \"boot_args\": \"console=ttyS0 reboot=k panic=1 pci=off\"}"

curl --unix-socket "$API_SOCK" -s -X PUT "http://localhost/drives/rootfs" \
    -H "Content-Type: application/json" \
    -d "{\"drive_id\": \"rootfs\", \"path_on_host\": \"$(realpath "${WORK_DIR}/rootfs.ext4")\", \"is_root_device\": true, \"is_read_only\": false}"

curl --unix-socket "$API_SOCK" -s -X PUT "http://localhost/vsock" \
    -H "Content-Type: application/json" \
    -d "{\"guest_cid\": 3, \"uds_path\": \"$(realpath "$VSOCK_SOCK")\"}"

curl --unix-socket "$API_SOCK" -s -X PUT "http://localhost/machine-config" \
    -H "Content-Type: application/json" \
    -d '{"vcpu_count": 2, "mem_size_mib": 2048}'

# Start the VM
curl --unix-socket "$API_SOCK" -s -X PUT "http://localhost/actions" \
    -H "Content-Type: application/json" \
    -d '{"action_type": "InstanceStart"}'

# Wait for guest agent to be ready
echo "Waiting for guest agent..."
for i in $(seq 1 60); do
    if echo '{"id":1,"method":"ping"}' | timeout 2 socat - "UNIX-CONNECT:${VSOCK_SOCK}" 2>/dev/null | grep -q "ok"; then
        echo "Guest agent ready after ${i}s"
        break
    fi
    sleep 1
done

# Pause the VM
curl --unix-socket "$API_SOCK" -s -X PATCH "http://localhost/vm" \
    -H "Content-Type: application/json" \
    -d '{"state": "Paused"}'

# Take snapshot
curl --unix-socket "$API_SOCK" -s -X PUT "http://localhost/snapshot/create" \
    -H "Content-Type: application/json" \
    -d "{\"snapshot_type\": \"Full\", \"snapshot_path\": \"$(realpath "${SNAPSHOT_DIR}/vmstate.bin")\", \"mem_file_path\": \"$(realpath "${SNAPSHOT_DIR}/memory.bin")\"}"

echo "Snapshot created at: ${SNAPSHOT_DIR}"

# Cleanup
kill $FC_PID 2>/dev/null || true
wait $FC_PID 2>/dev/null || true
rm -rf "$WORK_DIR"
```

Note: The snapshot bake script uses `socat` to ping the guest agent via vsock.
This is a simplified approach. If socat doesn't support vsock, use the
length-prefixed protocol manually or write a small helper binary.

Alternatively, skip the ping check and just sleep for 30 seconds after boot
to ensure the guest agent is fully initialized, then take the snapshot.

### Make all scripts executable

```bash
chmod +x artifacts/kernel/build.sh
chmod +x artifacts/rootfs/build.sh
chmod +x artifacts/guest-agent/build.sh
chmod +x artifacts/snapshot/bake.sh
```

## Testing Strategy

- Test `make guest-agent` builds on local machine
- Test `make all` on a KVM-enabled Linux machine (requires root for rootfs mount)
- Verify output structure: `output/{arch}/vmlinux`, `rootfs.ext4`, `guest-agent`, `snapshot/`
- Verify snapshot restores correctly with Firecracker

## Out of Scope

- Custom rootfs images
- Kernel modules
- Optimized kernel config per architecture (use generic configs)
- CI pipeline (Task K)
- Pre-built kernel/rootfs download from cache
