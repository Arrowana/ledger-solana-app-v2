#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL="${SPECULOS_MODEL:-nanosp}"
LEDGER_DEVICE="${LEDGER_DEVICE:-nanosplus}"
APP_ELF_HOST="${LEDGER_APP_ELF:-$ROOT_DIR/target/$LEDGER_DEVICE/release/ledger-squads-app}"
APP_ELF_CONTAINER="${LEDGER_APP_ELF_CONTAINER:-/app/target/$LEDGER_DEVICE/release/ledger-squads-app}"
DISPLAY_MODE="${SPECULOS_DISPLAY:-headless}"
APDU_PORT="${SPECULOS_APDU_PORT:-9999}"
API_PORT="${SPECULOS_API_PORT:-5000}"
AUTOMATION_PORT="${SPECULOS_AUTOMATION_PORT:-0}"
BUTTON_PORT="${SPECULOS_BUTTON_PORT:-0}"
VNC_PORT="${SPECULOS_VNC_PORT:-5900}"
IMAGE="${LEDGER_APP_BUILDER_IMAGE:-ghcr.io/ledgerhq/ledger-app-builder/ledger-app-dev-tools:latest}"
DOCKER_BIN="${DOCKER_BIN:-docker}"

if [ ! -f "$APP_ELF_HOST" ]; then
  echo >&2 "missing app ELF: $APP_ELF_HOST"
  echo >&2 "run './scripts/build-ledger.sh' first or set LEDGER_APP_ELF"
  exit 1
fi

if ! command -v "$DOCKER_BIN" >/dev/null 2>&1; then
  echo >&2 "docker is required to run Speculos"
  exit 1
fi

DOCKER_TTY_ARGS=()
if [ -t 0 ] && [ -t 1 ]; then
  DOCKER_TTY_ARGS=(-it)
fi

DOCKER_ARGS=(run --rm "${DOCKER_TTY_ARGS[@]}"
  -p "$APDU_PORT:$APDU_PORT"
  -p "$VNC_PORT:$VNC_PORT"
  -v "$ROOT_DIR:/app"
  --user "$(id -u):$(id -g)")

SPECULOS_ARGS=(speculos
  --model "$MODEL"
  --display "$DISPLAY_MODE"
  --apdu-port "$APDU_PORT"
  --vnc-port "$VNC_PORT")

if [ "$API_PORT" != "0" ]; then
  DOCKER_ARGS+=(-p "$API_PORT:$API_PORT")
  SPECULOS_ARGS+=(--api-port "$API_PORT")
fi

if [ "$AUTOMATION_PORT" != "0" ]; then
  DOCKER_ARGS+=(-p "$AUTOMATION_PORT:$AUTOMATION_PORT")
  SPECULOS_ARGS+=(--automation-port "$AUTOMATION_PORT")
fi

if [ "$BUTTON_PORT" != "0" ]; then
  DOCKER_ARGS+=(-p "$BUTTON_PORT:$BUTTON_PORT")
  SPECULOS_ARGS+=(--button-port "$BUTTON_PORT")
fi

SPECULOS_ARGS+=("$APP_ELF_CONTAINER")

exec "$DOCKER_BIN" "${DOCKER_ARGS[@]}" \
  "$IMAGE" \
  /bin/bash -lc "$(printf '%q ' "${SPECULOS_ARGS[@]}")"
