# Install Script + CI

## Context

The install script (`install.sh`) and GitHub Actions CI/CD pipelines. The CI
runs tests on every PR. The release pipeline builds all binaries + artifacts on
tagged releases and publishes them to GitHub Releases. The install script downloads
the latest release.

This is the final task — it ties everything together for distribution.

Depends on: FAU-76 (build pipeline), and assumes all crates compile.
See `docs/spec.md` for the install UX specification.

## Requirements

- `install.sh`: one-command setup for any KVM-enabled Linux machine
- GitHub Actions CI: test on every PR (cargo check, test, clippy, fmt)
- GitHub Actions release: build + publish on version tags
- Pre-built binaries for x86_64 and aarch64
- Pre-built artifacts tarballs for both architectures

## Implementation

### `install.sh` (root of repo)

```bash
#!/bin/sh
set -eu

# ============================================================
# AgentBox Installer
# Usage: curl -fsSL https://agentbox.dev/install.sh | sh
# ============================================================

REPO="fauzanazz/agentbox"
INSTALL_DIR="${AGENTBOX_INSTALL_DIR:-/usr/local/bin}"
DATA_DIR="${AGENTBOX_DATA_DIR:-/var/lib/agentbox}"
NEED_SUDO=""

main() {
    echo "AgentBox Installer"
    echo "=================="
    echo ""

    # 1. Detect architecture
    ARCH=$(uname -m)
    case "$ARCH" in
        x86_64)  ARCH_SUFFIX="x86_64" ;;
        aarch64) ARCH_SUFFIX="aarch64" ;;
        arm64)   ARCH_SUFFIX="aarch64" ;;  # macOS reports arm64
        *) error "Unsupported architecture: $ARCH" ;;
    esac
    echo "Architecture: $ARCH_SUFFIX"

    # 2. Detect OS
    OS=$(uname -s)
    if [ "$OS" != "Linux" ]; then
        error "AgentBox requires Linux with KVM. Detected: $OS
For macOS, use Lima to run a Linux VM with KVM:
  limactl create --name agentbox template://ubuntu
  limactl shell agentbox
Then run this installer inside the Lima VM."
    fi

    # 3. Verify KVM
    if [ ! -e /dev/kvm ]; then
        error "/dev/kvm not found. AgentBox requires KVM.
Options:
  - Run on bare-metal Linux
  - Use a cloud instance with KVM (AWS metal, GCP N2, Hetzner)
  - Enable nested virtualization in your hypervisor"
    fi

    if [ ! -r /dev/kvm ] || [ ! -w /dev/kvm ]; then
        echo "Warning: /dev/kvm exists but is not readable/writable."
        echo "You may need to add your user to the 'kvm' group:"
        echo "  sudo usermod -aG kvm \$USER"
        echo ""
    fi

    # 4. Check if we need sudo
    if [ ! -w "$INSTALL_DIR" ] || [ ! -w "$(dirname "$DATA_DIR")" ]; then
        NEED_SUDO="sudo"
        echo "Need sudo for installation to $INSTALL_DIR and $DATA_DIR"
    fi

    # 5. Get latest release version
    echo "Fetching latest release..."
    VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
        | grep '"tag_name"' | cut -d'"' -f4)

    if [ -z "$VERSION" ]; then
        error "Could not determine latest version. Check your internet connection."
    fi
    echo "Version: $VERSION"

    # 6. Download binary
    echo "Downloading agentbox binary..."
    BINARY_URL="https://github.com/$REPO/releases/download/$VERSION/agentbox-linux-$ARCH_SUFFIX"
    $NEED_SUDO mkdir -p "$INSTALL_DIR"
    curl -fsSL "$BINARY_URL" -o /tmp/agentbox
    $NEED_SUDO mv /tmp/agentbox "$INSTALL_DIR/agentbox"
    $NEED_SUDO chmod +x "$INSTALL_DIR/agentbox"
    echo "Binary installed: $INSTALL_DIR/agentbox"

    # 7. Download Firecracker
    echo "Downloading Firecracker..."
    FC_VERSION="v1.10.1"
    FC_URL="https://github.com/firecracker-microvm/firecracker/releases/download/${FC_VERSION}/firecracker-${FC_VERSION}-${ARCH_SUFFIX}.tgz"
    curl -fsSL "$FC_URL" | tar xz -C /tmp/
    $NEED_SUDO mv "/tmp/release-${FC_VERSION}-${ARCH_SUFFIX}/firecracker-${FC_VERSION}-${ARCH_SUFFIX}" "$DATA_DIR/firecracker" 2>/dev/null \
        || $NEED_SUDO mv /tmp/firecracker "$DATA_DIR/firecracker" 2>/dev/null \
        || true
    $NEED_SUDO chmod +x "$DATA_DIR/firecracker"
    echo "Firecracker installed: $DATA_DIR/firecracker"

    # 8. Download artifacts
    echo "Downloading VM artifacts (kernel, rootfs, snapshot)..."
    ARTIFACTS_URL="https://github.com/$REPO/releases/download/$VERSION/agentbox-artifacts-$ARCH_SUFFIX.tar.gz"
    $NEED_SUDO mkdir -p "$DATA_DIR"
    curl -fsSL "$ARTIFACTS_URL" | $NEED_SUDO tar xz -C "$DATA_DIR"
    echo "Artifacts installed to $DATA_DIR"

    # 9. Write default config
    if [ ! -f "$DATA_DIR/config.toml" ]; then
        $NEED_SUDO tee "$DATA_DIR/config.toml" > /dev/null <<CONFIGEOF
[daemon]
listen = "127.0.0.1:8080"
log_level = "info"

[vm]
firecracker_bin = "$DATA_DIR/firecracker"
kernel_path = "$DATA_DIR/vmlinux"
rootfs_path = "$DATA_DIR/rootfs.ext4"
snapshot_path = "$DATA_DIR/snapshot"

[vm.defaults]
memory_mb = 2048
vcpus = 2
network = false
timeout_secs = 3600

[pool]
min_size = 2
max_size = 10
idle_timeout_secs = 3600

[guest]
vsock_port = 5000
ping_timeout_ms = 5000
CONFIGEOF
        echo "Config written: $DATA_DIR/config.toml"
    else
        echo "Config exists, skipping: $DATA_DIR/config.toml"
    fi

    # 10. Install systemd service
    if command -v systemctl >/dev/null 2>&1; then
        $NEED_SUDO tee /etc/systemd/system/agentbox.service > /dev/null <<SERVICEEOF
[Unit]
Description=AgentBox Sandbox Daemon
After=network.target

[Service]
Type=simple
ExecStart=$INSTALL_DIR/agentbox serve --config $DATA_DIR/config.toml
Restart=always
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
SERVICEEOF
        $NEED_SUDO systemctl daemon-reload
        $NEED_SUDO systemctl enable --now agentbox
        echo "Systemd service installed and started."
    else
        echo "No systemd found. Start manually:"
        echo "  agentbox serve --config $DATA_DIR/config.toml"
    fi

    echo ""
    echo "============================================"
    echo "AgentBox $VERSION installed successfully!"
    echo "Daemon running on http://127.0.0.1:8080"
    echo ""
    echo "Next steps:"
    echo "  pip install agentbox    # Python SDK"
    echo "  # or"
    echo "  pnpm add agentbox      # TypeScript SDK"
    echo "============================================"
}

error() {
    echo "ERROR: $1" >&2
    exit 1
}

main "$@"
```

### `.github/workflows/ci.yml`

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - uses: Swatinem/rust-cache@v2

      - name: Check
        run: cargo check --workspace

      - name: Clippy
        run: cargo clippy --workspace -- -D warnings

      - name: Format
        run: cargo fmt --all -- --check

      - name: Test
        run: cargo test --workspace

  python-sdk:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: astral-sh/setup-uv@v4
      - name: Install and test
        working-directory: sdks/python
        run: |
          uv sync --group dev
          uv run pytest -v
```

### `.github/workflows/release.yml`

```yaml
name: Release

on:
  push:
    tags: ["v*"]

permissions:
  contents: write

jobs:
  build-binaries:
    strategy:
      matrix:
        include:
          - target: x86_64-unknown-linux-musl
            arch: x86_64
            os: ubuntu-latest
          - target: aarch64-unknown-linux-musl
            arch: aarch64
            os: ubuntu-latest
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - name: Install cross
        run: cargo install cross
      - name: Build agentbox CLI
        run: cross build --release --target ${{ matrix.target }} -p agentbox-cli
      - name: Build agentbox daemon
        run: cross build --release --target ${{ matrix.target }} -p agentbox-daemon
      - name: Build guest agent
        run: cross build --release --target ${{ matrix.target }} -p guest-agent
      - name: Package binaries
        run: |
          mkdir -p dist
          cp target/${{ matrix.target }}/release/agentbox dist/agentbox-linux-${{ matrix.arch }}
          cp target/${{ matrix.target }}/release/agentbox-daemon dist/agentbox-daemon-linux-${{ matrix.arch }}
          cp target/${{ matrix.target }}/release/guest-agent dist/guest-agent-${{ matrix.arch }}
      - uses: actions/upload-artifact@v4
        with:
          name: binaries-${{ matrix.arch }}
          path: dist/

  # Note: artifact building (kernel, rootfs, snapshot) requires KVM.
  # This would need a self-hosted runner with KVM access.
  # For MVP, artifacts can be built manually and uploaded to the release.

  release:
    needs: [build-binaries]
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/download-artifact@v4
        with:
          path: artifacts/
      - name: Create release
        uses: softprops/action-gh-release@v2
        with:
          files: |
            artifacts/binaries-x86_64/*
            artifacts/binaries-aarch64/*
          generate_release_notes: true

  publish-python:
    needs: [release]
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: astral-sh/setup-uv@v4
      - name: Build and publish
        working-directory: sdks/python
        env:
          UV_PUBLISH_TOKEN: ${{ secrets.PYPI_TOKEN }}
        run: |
          uv build
          uv publish
```

### Make install.sh executable

```bash
chmod +x install.sh
```

## Testing Strategy

### install.sh testing:
- Test on a clean Ubuntu 24.04 VM with KVM
- Verify all steps complete without errors
- Verify systemd service starts and daemon responds on :8080
- Test idempotency: run installer again, verify skips/updates

### CI testing:
- Push a PR, verify CI runs and passes
- Verify cargo check, clippy, fmt, and test all pass

### Release testing:
- Create a test tag, verify release pipeline runs
- Verify binaries are uploaded to GitHub Releases
- Run install.sh against the test release

## Out of Scope

- Homebrew formula
- Docker image distribution
- Automatic update mechanism (user re-runs install.sh)
- Artifact building in CI (needs KVM runner — document manual process)
- TypeScript SDK publishing to npm
