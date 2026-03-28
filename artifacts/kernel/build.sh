#!/usr/bin/env bash
set -euo pipefail

KERNEL_VERSION="$1"
ARCH="$2"
OUTPUT_DIR="$3"

MAJOR_VERSION=$(echo "$KERNEL_VERSION" | cut -d. -f1-2)
SRC_DIR="build/linux-${KERNEL_VERSION}"
VMLINUX="${OUTPUT_DIR}/vmlinux"

# Map to kernel ARCH name
case "$ARCH" in
    x86_64)  KERNEL_ARCH="x86_64" ;;
    aarch64) KERNEL_ARCH="arm64" ;;
    *) echo "ERROR: Unsupported architecture: $ARCH"; exit 1 ;;
esac

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

# Resolve dependencies for fragment config
make -C "$SRC_DIR" ARCH="$KERNEL_ARCH" olddefconfig

# Build
make -C "$SRC_DIR" ARCH="$KERNEL_ARCH" -j"$(nproc)" vmlinux

# Copy output
cp "${SRC_DIR}/vmlinux" "$VMLINUX"
echo "Kernel built: $VMLINUX"
