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
