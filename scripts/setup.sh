#!/bin/sh
# AgentBox build-from-source installer
# Usage: curl -fsSL https://raw.githubusercontent.com/fauzanazz/agentbox/main/scripts/setup.sh | sh
set -eu

REPO="fauzanazz/agentbox"
DATA_DIR="/var/lib/agentbox"
INSTALL_DIR="/usr/local/bin"
FC_VERSION="1.10.1"

info()  { printf "\033[1;34m==>\033[0m %s\n" "$1"; }
warn()  { printf "\033[1;33mWARN:\033[0m %s\n" "$1"; }
error() { printf "\033[1;31mERROR:\033[0m %s\n" "$1" >&2; exit 1; }

need_cmd() {
    if ! command -v "$1" > /dev/null 2>&1; then
        error "Required command '$1' not found. Please install it first."
    fi
}

# ── Checks ──
[ "$(uname -s)" = "Linux" ] || error "AgentBox requires Linux."
[ -e /dev/kvm ] || error "/dev/kvm not found. KVM is required."

ARCH="$(uname -m)"
case "$ARCH" in
    x86_64)  RUST_TARGET="x86_64-unknown-linux-musl" ;;
    aarch64) RUST_TARGET="aarch64-unknown-linux-musl" ;;
    *) error "Unsupported architecture: $ARCH" ;;
esac

need_cmd curl
need_cmd git
need_cmd make
need_cmd gcc
need_cmd socat

# ── Install Rust (if needed) ──
if ! command -v cargo > /dev/null 2>&1; then
    info "Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    . "$HOME/.cargo/env"
fi
rustup target add "$RUST_TARGET" 2>/dev/null || true

# ── Install Firecracker (if needed) ──
if ! command -v firecracker > /dev/null 2>&1; then
    info "Installing Firecracker v${FC_VERSION}..."
    tmp="$(mktemp -d)"
    curl -fsSL "https://github.com/firecracker-microvm/firecracker/releases/download/v${FC_VERSION}/firecracker-v${FC_VERSION}-${ARCH}.tgz" -o "${tmp}/fc.tgz"
    tar -xzf "${tmp}/fc.tgz" -C "$tmp"
    sudo install -m 755 "${tmp}/release-v${FC_VERSION}-${ARCH}/firecracker-v${FC_VERSION}-${ARCH}" "${INSTALL_DIR}/firecracker"
    rm -rf "$tmp"
fi

# ── Install build deps ──
info "Installing build dependencies..."
if command -v apt-get > /dev/null 2>&1; then
    sudo apt-get update -qq
    sudo apt-get install -y -qq build-essential flex bison libelf-dev bc libssl-dev musl-tools socat > /dev/null
elif command -v dnf > /dev/null 2>&1; then
    sudo dnf install -y gcc make flex bison elfutils-libelf-devel bc openssl-devel musl-tools socat
fi

# ── Clone and build ──
BUILD_DIR="/tmp/agentbox-build"
rm -rf "$BUILD_DIR"
info "Cloning repository..."
git clone --depth 1 "https://github.com/${REPO}.git" "$BUILD_DIR"
cd "$BUILD_DIR"

info "Building agentbox-daemon..."
cargo build --release -p agentbox-daemon

info "Building guest-agent..."
cargo build --release --target "$RUST_TARGET" -p guest-agent

sudo install -m 755 "target/release/agentbox-daemon" "${INSTALL_DIR}/agentbox-daemon"
mkdir -p "artifacts/output/${ARCH}"
cp "target/${RUST_TARGET}/release/guest-agent" "artifacts/output/${ARCH}/"

# ── Build kernel ──
info "Building kernel (this takes 5-15 minutes)..."
cd artifacts && make kernel ARCH="$ARCH"

# ── Build rootfs ──
info "Building rootfs..."
sudo make rootfs ARCH="$ARCH"

# ── Install to data dir ──
info "Installing artifacts to ${DATA_DIR}..."
sudo mkdir -p "${DATA_DIR}/snapshot"
sudo cp "output/${ARCH}/vmlinux" "${DATA_DIR}/vmlinux"
sudo cp "output/${ARCH}/rootfs.ext4" "${DATA_DIR}/rootfs.ext4"

# ── Bake snapshot ──
info "Baking snapshot (~30s)..."
cd /tmp
SNAP_DIR="$(mktemp -d)"
cp "${DATA_DIR}/rootfs.ext4" "${SNAP_DIR}/rootfs.ext4"
cd "$SNAP_DIR"

firecracker --api-sock api.sock 2>/dev/null &
FC_PID=$!
sleep 2

curl --unix-socket api.sock -sf -X PUT "http://localhost/boot-source" \
    -H "Content-Type: application/json" \
    -d "{\"kernel_image_path\": \"${DATA_DIR}/vmlinux\", \"boot_args\": \"console=ttyS0 reboot=k panic=1 pci=off\"}" > /dev/null
curl --unix-socket api.sock -sf -X PUT "http://localhost/drives/rootfs" \
    -H "Content-Type: application/json" \
    -d '{"drive_id":"rootfs","path_on_host":"rootfs.ext4","is_root_device":true,"is_read_only":false}' > /dev/null
curl --unix-socket api.sock -sf -X PUT "http://localhost/vsock" \
    -H "Content-Type: application/json" \
    -d '{"guest_cid":3,"uds_path":"vsock.sock"}' > /dev/null
curl --unix-socket api.sock -sf -X PUT "http://localhost/machine-config" \
    -H "Content-Type: application/json" \
    -d '{"vcpu_count":1,"mem_size_mib":512}' > /dev/null
curl --unix-socket api.sock -sf -X PUT "http://localhost/actions" \
    -H "Content-Type: application/json" \
    -d '{"action_type":"InstanceStart"}' > /dev/null

READY=false
for i in $(seq 1 60); do
    if echo -e "CONNECT 5000\n" | timeout 3 socat - "UNIX-CONNECT:vsock.sock" 2>/dev/null | grep -q "^OK"; then
        READY=true; break
    fi
    sleep 1
done

if [ "$READY" != "true" ]; then
    kill "$FC_PID" 2>/dev/null; rm -rf "$SNAP_DIR"
    error "Snapshot bake failed: guest agent did not respond"
fi

curl --unix-socket api.sock -sf -X PATCH "http://localhost/vm" \
    -H "Content-Type: application/json" -d '{"state":"Paused"}' > /dev/null
curl --unix-socket api.sock -sf -X PUT "http://localhost/snapshot/create" \
    -H "Content-Type: application/json" \
    -d "{\"snapshot_type\":\"Full\",\"snapshot_path\":\"${DATA_DIR}/snapshot/vmstate.bin\",\"mem_file_path\":\"${DATA_DIR}/snapshot/memory.bin\"}" > /dev/null

kill "$FC_PID" 2>/dev/null; wait "$FC_PID" 2>/dev/null || true
rm -rf "$SNAP_DIR"

# ── Write config ──
if [ ! -f "${DATA_DIR}/config.toml" ]; then
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
fi

# ── Systemd service ──
sudo tee /etc/systemd/system/agentbox.service > /dev/null <<'SERVICE'
[Unit]
Description=AgentBox Daemon
After=network.target

[Service]
Type=simple
ExecStart=/usr/local/bin/agentbox-daemon /var/lib/agentbox/config.toml
Restart=on-failure
RestartSec=5
LimitNOFILE=65535
TimeoutStopSec=10

[Install]
WantedBy=multi-user.target
SERVICE

sudo systemctl daemon-reload
sudo systemctl enable agentbox
sudo systemctl start agentbox

# ── Cleanup ──
rm -rf "$BUILD_DIR"

printf "\n"
info "AgentBox installed and running!"
printf "\n"
printf "  Health check:  curl http://localhost:8080/health\n"
printf "  Pool status:   curl http://localhost:8080/pool/status\n"
printf "  Config:        ${DATA_DIR}/config.toml\n"
printf "  Logs:          journalctl -u agentbox -f\n"
printf "\n"
