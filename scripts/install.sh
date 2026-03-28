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

    info "Downloading VM artifacts (kernel, rootfs)..."
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

install_socat() {
    if command -v socat > /dev/null 2>&1; then return; fi
    info "Installing socat..."
    if command -v apt-get > /dev/null 2>&1; then
        sudo apt-get update -qq && sudo apt-get install -y -qq socat > /dev/null
    elif command -v dnf > /dev/null 2>&1; then
        sudo dnf install -y socat > /dev/null
    elif command -v apk > /dev/null 2>&1; then
        sudo apk add socat > /dev/null
    else
        warn "Could not install socat automatically. Please install it manually."
    fi
}

bake_snapshot() {
    if [ -f "${DATA_DIR}/snapshot/vmstate.bin" ]; then
        info "Snapshot already exists"
        return
    fi

    if [ ! -f "${DATA_DIR}/vmlinux" ] || [ ! -f "${DATA_DIR}/rootfs.ext4" ]; then
        warn "Kernel or rootfs not found — cannot bake snapshot"
        return
    fi

    need_cmd socat
    need_cmd firecracker

    info "Baking VM snapshot (this takes ~30s)..."

    local work_dir="$(mktemp -d)"
    cp "${DATA_DIR}/rootfs.ext4" "${work_dir}/rootfs.ext4"

    cd "$work_dir"
    firecracker --api-sock api.sock &
    local fc_pid=$!
    trap "kill $fc_pid 2>/dev/null || true; rm -rf $work_dir" RETURN
    sleep 2

    local vmlinux="$(realpath "${DATA_DIR}/vmlinux")"

    curl --unix-socket api.sock -sf -X PUT "http://localhost/boot-source" \
        -H "Content-Type: application/json" \
        -d "{\"kernel_image_path\": \"${vmlinux}\", \"boot_args\": \"console=ttyS0 reboot=k panic=1 pci=off\"}" > /dev/null

    curl --unix-socket api.sock -sf -X PUT "http://localhost/drives/rootfs" \
        -H "Content-Type: application/json" \
        -d '{"drive_id": "rootfs", "path_on_host": "rootfs.ext4", "is_root_device": true, "is_read_only": false}' > /dev/null

    curl --unix-socket api.sock -sf -X PUT "http://localhost/vsock" \
        -H "Content-Type: application/json" \
        -d '{"guest_cid": 3, "uds_path": "vsock.sock"}' > /dev/null

    curl --unix-socket api.sock -sf -X PUT "http://localhost/machine-config" \
        -H "Content-Type: application/json" \
        -d '{"vcpu_count": 1, "mem_size_mib": 512}' > /dev/null

    curl --unix-socket api.sock -sf -X PUT "http://localhost/actions" \
        -H "Content-Type: application/json" \
        -d '{"action_type": "InstanceStart"}' > /dev/null

    # Wait for guest agent via Firecracker CONNECT handshake
    local ready=false
    for i in $(seq 1 60); do
        if printf "CONNECT 5000\n" | timeout 3 socat - "UNIX-CONNECT:vsock.sock" 2>/dev/null | grep -q "^OK"; then
            ready=true
            break
        fi
        sleep 1
    done

    if [ "$ready" != "true" ]; then
        kill "$fc_pid" 2>/dev/null || true
        rm -rf "$work_dir"
        error "Guest agent did not start. Snapshot bake failed."
    fi

    sudo mkdir -p "${DATA_DIR}/snapshot"

    curl --unix-socket api.sock -sf -X PATCH "http://localhost/vm" \
        -H "Content-Type: application/json" \
        -d '{"state": "Paused"}' > /dev/null

    curl --unix-socket api.sock -sf -X PUT "http://localhost/snapshot/create" \
        -H "Content-Type: application/json" \
        -d "{\"snapshot_type\": \"Full\", \"snapshot_path\": \"${DATA_DIR}/snapshot/vmstate.bin\", \"mem_file_path\": \"${DATA_DIR}/snapshot/memory.bin\"}" > /dev/null

    kill "$fc_pid" 2>/dev/null || true
    wait "$fc_pid" 2>/dev/null || true
    rm -rf "$work_dir"

    info "Snapshot baked successfully"
}

setup_config() {
    if [ -f "${DATA_DIR}/config.toml" ]; then
        return
    fi
    sudo tee "${DATA_DIR}/config.toml" > /dev/null <<'CONFIG'
[daemon]
listen = "0.0.0.0:8080"
log_level = "info"

[vm]
firecracker_bin = "/usr/local/bin/firecracker"
kernel_path = "/var/lib/agentbox/vmlinux"
rootfs_path = "/var/lib/agentbox/rootfs.ext4"
snapshot_path = "/var/lib/agentbox/snapshot"

[vm.defaults]
memory_mb = 512
vcpus = 1
network = false
timeout_secs = 3600

[pool]
min_size = 2
max_size = 10
idle_timeout_secs = 3600

[guest]
vsock_port = 5000
ping_timeout_ms = 15000
CONFIG
    info "Config written to ${DATA_DIR}/config.toml"
}

setup_systemd() {
    if ! command -v systemctl > /dev/null 2>&1; then
        return
    fi

    if [ -f /etc/systemd/system/agentbox.service ]; then
        return
    fi

    info "Installing systemd service..."
    sudo tee /etc/systemd/system/agentbox.service > /dev/null <<'SERVICE'
[Unit]
Description=AgentBox Daemon
After=network.target
Wants=network.target

[Service]
Type=simple
ExecStart=/usr/local/bin/agentbox-daemon /var/lib/agentbox/config.toml
Restart=on-failure
RestartSec=5
LimitNOFILE=65536
TimeoutStopSec=45

# Security hardening
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths=/var/lib/agentbox /tmp
ProtectKernelTunables=yes
ProtectControlGroups=yes

# Resource limits (adjust based on expected workload)
MemoryMax=8G
CPUQuota=400%

[Install]
WantedBy=multi-user.target
SERVICE

    sudo systemctl daemon-reload
    sudo systemctl enable agentbox.service
    sudo systemctl start agentbox.service
    info "AgentBox service started"
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
    install_socat
    bake_snapshot
    setup_config
    setup_systemd

    printf "\n"
    info "AgentBox ${VERSION} installed successfully!"
    printf "\n"
    printf "  Service management:\n"
    printf "    sudo systemctl status agentbox   # Check status\n"
    printf "    sudo systemctl restart agentbox  # Restart\n"
    printf "    sudo journalctl -u agentbox -f   # View logs\n"
    printf "\n"
    printf "  Config: ${DATA_DIR}/config.toml\n"
    printf "\n"
    printf "  Install the Python SDK:\n"
    printf "    pip install agentbox\n"
    printf "\n"
}

main
