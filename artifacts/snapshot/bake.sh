#!/usr/bin/env bash
set -euo pipefail

if [ $# -lt 2 ]; then
    echo "Usage: $0 <arch> <output-dir> [--rootfs <name>]"
    exit 1
fi

ARCH="$1"
OUTPUT_DIR="$2"

ROOTFS_NAME=""
if [ "${3:-}" = "--rootfs" ] && [ -n "${4:-}" ]; then
    ROOTFS_NAME="$4"
fi

if [ -n "$ROOTFS_NAME" ]; then
    SNAPSHOT_DIR="$(realpath "${OUTPUT_DIR}")/snapshot-${ROOTFS_NAME}"
    ROOTFS="$(realpath "${OUTPUT_DIR}/rootfs-${ROOTFS_NAME}.ext4")"
else
    SNAPSHOT_DIR="$(realpath "${OUTPUT_DIR}")/snapshot"
    ROOTFS="$(realpath "${OUTPUT_DIR}/rootfs.ext4")"
fi

VMLINUX="$(realpath "${OUTPUT_DIR}/vmlinux")"
WORK_DIR="$(pwd)/build/snapshot-bake"

if [ -f "${SNAPSHOT_DIR}/vmstate.bin" ]; then
    echo "Snapshot already baked: ${SNAPSHOT_DIR}"
    exit 0
fi

if [ -n "$ROOTFS_NAME" ]; then
    echo "Baking snapshot for template '${ROOTFS_NAME}'..."
else
    echo "Baking base snapshot..."
fi

mkdir -p "$SNAPSHOT_DIR" "$WORK_DIR"

# Copy rootfs (don't modify the original)
cp "$ROOTFS" "${WORK_DIR}/rootfs.ext4"

# Firecracker embeds drive and vsock UDS paths into the snapshot.
# Use relative paths so that restored snapshots work from any temp directory.
# The kernel path is NOT embedded, so it can stay absolute.
cd "$WORK_DIR"

# Start Firecracker (fresh boot) — relative api.sock inside WORK_DIR
firecracker --api-sock api.sock &
FC_PID=$!

# Ensure cleanup on exit (use absolute WORK_DIR path since we cd'd)
cleanup() {
    kill "$FC_PID" 2>/dev/null || true
    wait "$FC_PID" 2>/dev/null || true
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

sleep 1

# Configure VM via API — kernel uses absolute path (not embedded in snapshot),
# rootfs and vsock use relative paths (embedded in snapshot).
curl --unix-socket api.sock -sf -X PUT "http://localhost/boot-source" \
    -H "Content-Type: application/json" \
    -d "{\"kernel_image_path\": \"${VMLINUX}\", \"boot_args\": \"console=ttyS0 reboot=k panic=1 pci=off\"}"

curl --unix-socket api.sock -sf -X PUT "http://localhost/drives/rootfs" \
    -H "Content-Type: application/json" \
    -d '{"drive_id": "rootfs", "path_on_host": "rootfs.ext4", "is_root_device": true, "is_read_only": false}'

curl --unix-socket api.sock -sf -X PUT "http://localhost/vsock" \
    -H "Content-Type: application/json" \
    -d '{"guest_cid": 3, "uds_path": "vsock.sock"}'

curl --unix-socket api.sock -sf -X PUT "http://localhost/machine-config" \
    -H "Content-Type: application/json" \
    -d '{"vcpu_count": 1, "mem_size_mib": 512}'

# Start the VM
curl --unix-socket api.sock -sf -X PUT "http://localhost/actions" \
    -H "Content-Type: application/json" \
    -d '{"action_type": "InstanceStart"}'

# Wait for guest agent to be ready via vsock CONNECT handshake
echo "Waiting for guest agent..."
AGENT_READY=false
for i in $(seq 1 60); do
    if printf "CONNECT 5000\n" | timeout 2 socat - UNIX-CONNECT:vsock.sock 2>/dev/null | grep -q "^OK"; then
        echo "Guest agent ready after ${i}s"
        AGENT_READY=true
        break
    fi
    sleep 1
done

if [ "$AGENT_READY" != "true" ]; then
    echo "ERROR: Guest agent did not become ready within 60s"
    exit 1
fi

# Pause the VM
curl --unix-socket api.sock -sf -X PATCH "http://localhost/vm" \
    -H "Content-Type: application/json" \
    -d '{"state": "Paused"}'

# Take snapshot — output paths are absolute (not embedded in snapshot config)
curl --unix-socket api.sock -sf -X PUT "http://localhost/snapshot/create" \
    -H "Content-Type: application/json" \
    -d "{\"snapshot_type\": \"Full\", \"snapshot_path\": \"${SNAPSHOT_DIR}/vmstate.bin\", \"mem_file_path\": \"${SNAPSHOT_DIR}/memory.bin\"}"

echo "Snapshot created at: ${SNAPSHOT_DIR}"
