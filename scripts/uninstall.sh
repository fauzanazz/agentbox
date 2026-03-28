#!/usr/bin/env bash
set -euo pipefail

# AgentBox uninstall script
# Usage: ./uninstall.sh [--purge]
#   --purge: also remove /var/lib/agentbox (config, snapshots, rootfs)

PURGE=false
for arg in "$@"; do
    case "$arg" in
        --purge) PURGE=true ;;
        -h|--help)
            echo "Usage: $0 [--purge]"
            echo "  --purge  Remove all data in /var/lib/agentbox (config, snapshots, rootfs)"
            exit 0
            ;;
        *)
            echo "Unknown option: $arg" >&2
            exit 1
            ;;
    esac
done

info()  { printf "\033[1;34m[info]\033[0m  %s\n" "$1"; }
warn()  { printf "\033[1;33m[warn]\033[0m  %s\n" "$1"; }
error() { printf "\033[1;31m[error]\033[0m %s\n" "$1"; }

if [ "$(id -u)" -ne 0 ]; then
    error "This script must be run as root (or with sudo)"
    exit 1
fi

# Stop and disable the systemd service
if systemctl is-active --quiet agentbox.service 2>/dev/null; then
    info "Stopping agentbox service..."
    systemctl stop agentbox.service
fi

if systemctl is-enabled --quiet agentbox.service 2>/dev/null; then
    info "Disabling agentbox service..."
    systemctl disable agentbox.service
fi

if [ -f /etc/systemd/system/agentbox.service ]; then
    info "Removing systemd unit file..."
    rm -f /etc/systemd/system/agentbox.service
    systemctl daemon-reload
fi

# Remove binaries
for bin in agentbox-daemon agentbox-cli; do
    if [ -f "/usr/local/bin/$bin" ]; then
        info "Removing /usr/local/bin/$bin"
        rm -f "/usr/local/bin/$bin"
    fi
done

# Remove firecracker if installed by us
if [ -f /usr/local/bin/firecracker ]; then
    info "Removing /usr/local/bin/firecracker"
    rm -f /usr/local/bin/firecracker
fi

# Purge data directory
if [ "$PURGE" = true ]; then
    if [ -d /var/lib/agentbox ]; then
        warn "Removing /var/lib/agentbox (all data, config, snapshots)..."
        rm -rf /var/lib/agentbox
    fi
else
    info "Data directory /var/lib/agentbox preserved (use --purge to remove)"
fi

info "AgentBox uninstalled successfully"
