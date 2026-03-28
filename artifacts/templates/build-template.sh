#!/usr/bin/env bash
set -euo pipefail

if [ $# -lt 3 ]; then
    echo "Usage: $0 <template-name> <arch> <output-dir>"
    exit 1
fi

TEMPLATE_NAME="$1"
ARCH="$2"
OUTPUT_DIR="$3"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TEMPLATE_DIR="${SCRIPT_DIR}/${TEMPLATE_NAME}"

if [ ! -d "$TEMPLATE_DIR" ]; then
    echo "Error: template directory not found: $TEMPLATE_DIR"
    exit 1
fi

BASE_ROOTFS="${OUTPUT_DIR}/rootfs.ext4"
TEMPLATE_ROOTFS="${OUTPUT_DIR}/rootfs-${TEMPLATE_NAME}.ext4"
MOUNT_DIR="build/rootfs-${TEMPLATE_NAME}-mount"

if [ -f "$TEMPLATE_ROOTFS" ]; then
    echo "Template rootfs already built: $TEMPLATE_ROOTFS"
    exit 0
fi

if [ ! -f "$BASE_ROOTFS" ]; then
    echo "Error: base rootfs not found: $BASE_ROOTFS"
    exit 1
fi

echo "Building template '${TEMPLATE_NAME}' rootfs for ${ARCH}..."

# Copy base rootfs (use reflink if supported, else plain copy)
cp --reflink=auto "$BASE_ROOTFS" "$TEMPLATE_ROOTFS"

# Mount the copy
mkdir -p "$MOUNT_DIR"
sudo mount -o loop "$TEMPLATE_ROOTFS" "$MOUNT_DIR"

# Ensure cleanup on exit
cleanup() {
    if mountpoint -q "$MOUNT_DIR" 2>/dev/null; then
        sudo umount "$MOUNT_DIR"
    fi
    [ -d "$MOUNT_DIR" ] && rmdir "$MOUNT_DIR" 2>/dev/null || true
}
trap cleanup EXIT

# Ensure /workspace exists in the rootfs
sudo mkdir -p "${MOUNT_DIR}/workspace"

# Copy template files into /workspace (excluding setup.sh)
for f in "${TEMPLATE_DIR}"/*; do
    fname="$(basename "$f")"
    if [ "$fname" = "setup.sh" ]; then
        continue
    fi
    sudo cp "$f" "${MOUNT_DIR}/workspace/${fname}"
done

# Run setup.sh inside chroot if present
if [ -f "${TEMPLATE_DIR}/setup.sh" ]; then
    echo "Running setup.sh for template '${TEMPLATE_NAME}'..."
    sudo cp "${TEMPLATE_DIR}/setup.sh" "${MOUNT_DIR}/workspace/setup.sh"
    sudo chmod +x "${MOUNT_DIR}/workspace/setup.sh"
    # Copy host DNS config so package managers can resolve hosts
    sudo cp /etc/resolv.conf "${MOUNT_DIR}/etc/resolv.conf" 2>/dev/null || true
    sudo chroot "$MOUNT_DIR" /workspace/setup.sh
    sudo rm -f "${MOUNT_DIR}/workspace/setup.sh"
fi

# Unmount cleanly (trap handles failure case)
sudo umount "$MOUNT_DIR"
rmdir "$MOUNT_DIR"
trap - EXIT

echo "Template rootfs built: $TEMPLATE_ROOTFS"
