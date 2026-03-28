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

MIRROR="https://dl-cdn.alpinelinux.org/alpine/v${ALPINE_VERSION}"

# Ensure apk is available (not present on Ubuntu/Debian)
if ! command -v apk > /dev/null 2>&1; then
    echo "Installing apk-tools-static..."
    APK_PKG=$(curl -fsSL "${MIRROR}/main/${ALPINE_ARCH}/" | grep -o 'apk-tools-static-[^"]*\.apk' | head -1)
    curl -fsSL "${MIRROR}/main/${ALPINE_ARCH}/${APK_PKG}" -o /tmp/apk-static.apk
    tar xzf /tmp/apk-static.apk -C /tmp 2>/dev/null || true
    sudo install -m 755 /tmp/sbin/apk.static /usr/local/bin/apk
    rm -f /tmp/apk-static.apk
fi

# Create ext4 image
dd if=/dev/zero of="$ROOTFS" bs=1M count="$SIZE_MB"
mkfs.ext4 -F "$ROOTFS"

# Mount
mkdir -p "$MOUNT_DIR"
sudo mount -o loop "$ROOTFS" "$MOUNT_DIR"

# Ensure cleanup on exit
cleanup() {
    if mountpoint -q "$MOUNT_DIR" 2>/dev/null; then
        sudo umount "$MOUNT_DIR"
    fi
    [ -d "$MOUNT_DIR" ] && rmdir "$MOUNT_DIR" 2>/dev/null || true
}
trap cleanup EXIT

# Bootstrap Alpine
sudo apk -X "${MIRROR}/main" -U --allow-untrusted --root "$MOUNT_DIR" \
    --initdb add alpine-base

# Configure repos
sudo mkdir -p "${MOUNT_DIR}/etc/apk"
echo "${MIRROR}/main" | sudo tee "${MOUNT_DIR}/etc/apk/repositories"
echo "${MIRROR}/community" | sudo tee -a "${MOUNT_DIR}/etc/apk/repositories"

# Install packages (retry on transient CDN errors)
for attempt in 1 2 3; do
    if sudo chroot "$MOUNT_DIR" apk update --allow-untrusted 2>&1; then
        break
    fi
    echo "apk update attempt $attempt failed, retrying in 5s..."
    sleep 5
done
sudo chroot "$MOUNT_DIR" apk add --allow-untrusted --no-cache \
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

# Unmount (trap handles cleanup on failure, but do it explicitly on success)
sudo umount "$MOUNT_DIR"
rmdir "$MOUNT_DIR"
trap - EXIT

echo "Rootfs built: $ROOTFS"
