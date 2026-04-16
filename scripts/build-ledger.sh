#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_DIR="$ROOT_DIR/ledger-app"
TARGET_DIR="$ROOT_DIR/target"
IMAGE="${LEDGER_APP_BUILDER_IMAGE:-ghcr.io/ledgerhq/ledger-app-builder/ledger-app-dev-tools:latest}"
LEDGER_DEVICE="${LEDGER_DEVICE:-nanosplus}"
DOCKER_BIN="${DOCKER_BIN:-docker}"

if ! command -v "$DOCKER_BIN" >/dev/null 2>&1; then
  echo >&2 "docker is required to build the Ledger app"
  exit 1
fi

if [ ! -d "$APP_DIR" ] || [ ! -f "$APP_DIR/Cargo.toml" ]; then
  echo >&2 "ledger-app directory is missing or incomplete: $APP_DIR"
  exit 1
fi

echo "Building Ledger app with $IMAGE for $LEDGER_DEVICE"

DOCKER_TTY_ARGS=()
if [ -t 0 ] && [ -t 1 ]; then
  DOCKER_TTY_ARGS=(-t)
fi

"$DOCKER_BIN" run --rm "${DOCKER_TTY_ARGS[@]}" \
  -v "$ROOT_DIR:/app" \
  --user "$(id -u):$(id -g)" \
  -e LEDGER_DEVICE="$LEDGER_DEVICE" \
  "$IMAGE" \
  /bin/bash -lc '
set -euo pipefail
export PATH="/opt/.cargo/bin:$PATH"
cd /app/ledger-app
cargo ledger build "$LEDGER_DEVICE"
'

echo "Ledger app build complete: $TARGET_DIR/$LEDGER_DEVICE/release/ledger-squads-app"
