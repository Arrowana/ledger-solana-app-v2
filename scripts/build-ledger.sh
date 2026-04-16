#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_DIR="$ROOT_DIR/ledger-app"
IMAGE="${LEDGER_APP_BUILDER_IMAGE:-ghcr.io/ledgerhq/ledger-app-builder/ledger-app-dev-tools:latest}"
SDK_ENV_VAR="${LEDGER_SDK_ENV_VAR:-NANOSP_SDK}"
DOCKER_BIN="${DOCKER_BIN:-docker}"

if ! command -v "$DOCKER_BIN" >/dev/null 2>&1; then
  echo >&2 "docker is required for bun run build:ledger"
  exit 1
fi

if [ ! -d "$APP_DIR" ] || [ ! -f "$APP_DIR/Makefile" ]; then
  echo >&2 "ledger-app directory is missing or incomplete: $APP_DIR"
  exit 1
fi

echo "Building Ledger app with $IMAGE using SDK env $SDK_ENV_VAR"

DOCKER_TTY_ARGS=()
if [ -t 0 ] && [ -t 1 ]; then
  DOCKER_TTY_ARGS=(-t)
fi

"$DOCKER_BIN" run --rm "${DOCKER_TTY_ARGS[@]}" \
  -v "$ROOT_DIR:/app" \
  --user "$(id -u):$(id -g)" \
  -e SDK_ENV_VAR="$SDK_ENV_VAR" \
  "$IMAGE" \
  /bin/bash -lc '
set -euo pipefail

BUILD_ROOT=/app/.ledger-build
rm -rf "$BUILD_ROOT"
mkdir -p "$BUILD_ROOT"
trap '\''rm -rf "$BUILD_ROOT"'\'' EXIT
cp -R /app/ledger-app "$BUILD_ROOT/ledger-app"

cd "$BUILD_ROOT"
git init -q
cd ledger-app

SDK_PATH="${!SDK_ENV_VAR:-}"
if [ -z "$SDK_PATH" ]; then
  echo >&2 "Missing SDK path in container: $SDK_ENV_VAR"
  exit 1
fi

BOLOS_SDK="$SDK_PATH" make clean
BOLOS_SDK="$SDK_PATH" make

rm -rf /app/ledger-app/build
cp -R build /app/ledger-app/build
'

echo "Ledger app build complete: $APP_DIR/build/nanos2/bin/app.elf"
