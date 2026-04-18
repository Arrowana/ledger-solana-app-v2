#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_DIR="$ROOT_DIR/ledger-app"
TARGET_DIR="$ROOT_DIR/target"
IMAGE="${LEDGER_APP_BUILDER_IMAGE:-ghcr.io/ledgerhq/ledger-app-builder/ledger-app-dev-tools:latest}"
LEDGER_DEVICE="${LEDGER_DEVICE:-nanosplus}"
DOCKER_BIN="${DOCKER_BIN:-docker}"
APP_BIN="${LEDGER_APP_BIN:-ledger-solana-app-v2}"
MAX_SIZE="${LEDGER_APP_MAX_SIZE:-${LEDGER_APP_MAX_DEC:-}}"

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

BUILD_LOG="$(mktemp)"
cleanup() {
  rm -f "$BUILD_LOG"
}
trap cleanup EXIT

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
' | tee "$BUILD_LOG"

REPORT_DIR="$TARGET_DIR/$LEDGER_DEVICE/release"
REPORT_TXT="$REPORT_DIR/$APP_BIN.size.txt"
REPORT_JSON="$REPORT_DIR/$APP_BIN.size.json"
SIZE_TARGET="/app/target/$LEDGER_DEVICE/release/$APP_BIN"
SIZE_LINE="$(awk -v target="$SIZE_TARGET" '$NF == target { line = $0 } END { print line }' "$BUILD_LOG")"

if [ -z "$SIZE_LINE" ]; then
  echo >&2 "warning: failed to extract size summary for $APP_BIN from build output"
else
  read -r TEXT_SIZE DATA_SIZE BSS_SIZE DEC_SIZE HEX_SIZE _ <<< "$SIZE_LINE"

  mkdir -p "$REPORT_DIR"
  cat > "$REPORT_TXT" <<EOF
device=$LEDGER_DEVICE
app=$APP_BIN
size_bytes=$DEC_SIZE
EOF

  cat > "$REPORT_JSON" <<EOF
{
  "device": "$LEDGER_DEVICE",
  "app": "$APP_BIN",
  "size_bytes": $DEC_SIZE
}
EOF

  echo "Size summary saved to $REPORT_TXT and $REPORT_JSON"

  if [ -n "$MAX_SIZE" ] && [ "$DEC_SIZE" -gt "$MAX_SIZE" ]; then
    echo >&2 "app size check failed: size_bytes=$DEC_SIZE exceeds LEDGER_APP_MAX_SIZE=$MAX_SIZE"
    exit 1
  fi
fi

echo "Ledger app build complete: $TARGET_DIR/$LEDGER_DEVICE/release/$APP_BIN"
