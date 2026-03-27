#!/bin/sh
# AgentBox installer — curl -fsSL https://raw.githubusercontent.com/fauzanazz/agentbox/main/scripts/install.sh | sh
set -eu

REPO="fauzanazz/agentbox"
INSTALL_DIR="/usr/local/bin"
DATA_DIR="/var/lib/agentbox"
FC_VERSION="1.10.1"
VERSION=""

# ── Helpers ────────────────────────────────────────────────────

info()  { printf "\033[1;34m==>\033[0m %s\n" "$1"; }
warn()  { printf "\033[1;33mWARN:\033[0m %s\n" "$1"; }
error() { printf "\033[1;31mERROR:\033[0m %s\n" "$1" >&2; exit 1; }

need_cmd() {
    if ! command -v "$1" > /dev/null 2>&1; then
        error "Required command '$1' not found. Please install it first."
    fi
}

# ── Parse args ─────────────────────────────────────────────────

while [ $# -gt 0 ]; do
    case "$1" in
        --version) VERSION="$2"; shift 2 ;;
        --version=*) VERSION="${1#*=}"; shift ;;
        *) error "Unknown option: $1" ;;
    esac
done

# ── Detection ──────────────────────────────────────────────────

detect_arch() {
    ARCH="$(uname -m)"
    case "$ARCH" in
        x86_64)  ARCH="x86_64" ;;
        aarch64) ARCH="aarch64" ;;
        arm64)   ARCH="aarch64" ;;
        *) error "Unsupported architecture: $ARCH (only x86_64 and aarch64 are supported)" ;;
    esac
}

detect_os() {
    OS="$(uname -s)"
    case "$OS" in
        Linux) ;;
        Darwin)
            error "AgentBox requires Linux with KVM. On macOS, use Lima or a Linux VM:
  brew install lima
  limactl start --name=agentbox template://default
  limactl shell agentbox
  curl -fsSL https://raw.githubusercontent.com/${REPO}/main/scripts/install.sh | sh"
            ;;
        *) error "Unsupported OS: $OS (only Linux is supported)" ;;
    esac
}

check_kvm() {
    if [ ! -e /dev/kvm ]; then
        error "/dev/kvm not found. AgentBox requires KVM. Enable nested virtualization or use a bare-metal host."
    fi
    if [ ! -r /dev/kvm ] || [ ! -w /dev/kvm ]; then
        warn "/dev/kvm exists but is not readable/writable. You may need: sudo chmod 666 /dev/kvm"
    fi
}

# ── Version resolution ─────────────────────────────────────────

resolve_version() {
    if [ -n "$VERSION" ]; then
        # Ensure 'v' prefix
        case "$VERSION" in
            v*) ;;
            *) VERSION="v${VERSION}" ;;
        esac
        return
    fi

    need_cmd curl
    info "Fetching latest release version..."
    VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | \
        grep '"tag_name"' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')

    if [ -z "$VERSION" ]; then
        error "Failed to determine latest version. Use --version to specify."
    fi
}

# ── Installation ───────────────────────────────────────────────

download_binary() {
    local url="https://github.com/${REPO}/releases/download/${VERSION}/agentbox-${VERSION}-linux-${ARCH}.tar.gz"
    local tmp="$(mktemp -d)"

    info "Downloading AgentBox ${VERSION} for ${ARCH}..."
    curl -fsSL "$url" -o "${tmp}/agentbox.tar.gz" || \
        error "Failed to download from ${url}. Check that the release exists."

    tar -xzf "${tmp}/agentbox.tar.gz" -C "${tmp}"

    info "Installing to ${INSTALL_DIR}..."
    sudo install -m 755 "${tmp}/agentbox" "${INSTALL_DIR}/agentbox"
    sudo install -m 755 "${tmp}/agentbox-daemon" "${INSTALL_DIR}/agentbox-daemon"

    rm -rf "${tmp}"
}

download_firecracker() {
    if command -v firecracker > /dev/null 2>&1; then
        info "Firecracker already installed: $(which firecracker)"
        return
    fi

    local fc_arch="$ARCH"
    local url="https://github.com/firecracker-microvm/firecracker/releases/download/v${FC_VERSION}/firecracker-v${FC_VERSION}-${fc_arch}.tgz"
    local tmp="$(mktemp -d)"

    info "Downloading Firecracker v${FC_VERSION}..."
    curl -fsSL "$url" -o "${tmp}/firecracker.tgz" || \
        error "Failed to download Firecracker."

    tar -xzf "${tmp}/firecracker.tgz" -C "${tmp}"
    sudo install -m 755 "${tmp}/release-v${FC_VERSION}-${fc_arch}/firecracker-v${FC_VERSION}-${fc_arch}" \
        "${INSTALL_DIR}/firecracker"

    rm -rf "${tmp}"
    info "Firecracker installed to ${INSTALL_DIR}/firecracker"
}

setup_data_dir() {
    if [ ! -d "$DATA_DIR" ]; then
        info "Creating ${DATA_DIR}..."
        sudo mkdir -p "$DATA_DIR"
    fi
}

download_artifacts() {
    local url="https://github.com/${REPO}/releases/download/${VERSION}/agentbox-artifacts-${ARCH}.tar.gz"
    local tmp="$(mktemp -d)"

    info "Downloading VM artifacts (kernel, rootfs, snapshot)..."
    if curl -fsSL "$url" -o "${tmp}/artifacts.tar.gz" 2>/dev/null; then
        sudo tar -xzf "${tmp}/artifacts.tar.gz" -C "${DATA_DIR}"
        rm -rf "${tmp}"
        info "Artifacts installed to ${DATA_DIR}"
    else
        warn "VM artifacts not found in release. You may need to build them manually:"
        warn "  cd artifacts && make all"
        rm -rf "${tmp}"
    fi
}

# ── Main ───────────────────────────────────────────────────────

main() {
    need_cmd curl
    need_cmd tar
    need_cmd sudo

    detect_arch
    detect_os
    check_kvm
    resolve_version

    info "Installing AgentBox ${VERSION} (${ARCH})"

    download_binary
    download_firecracker
    setup_data_dir
    download_artifacts

    printf "\n"
    info "AgentBox ${VERSION} installed successfully!"
    printf "\n"
    printf "  Start the daemon:\n"
    printf "    agentbox serve\n"
    printf "\n"
    printf "  Install the Python SDK:\n"
    printf "    pip install agentbox\n"
    printf "\n"
}

main
