#!/usr/bin/env bash
set -euo pipefail
# Install Node.js dependencies into /workspace
cd /workspace
npm install --no-audit --no-fund
