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
