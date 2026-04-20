#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IDLS_DIR="$ROOT_DIR/idls"
PRUNE_SCRIPT="$ROOT_DIR/scripts/prune-codama.jq"

mkdir -p "$IDLS_DIR"

fetch_and_prune() {
  local name="$1"
  local url="$2"

  echo "Updating $name from $url"
  curl -fsSL "$url" | jq -c -f "$PRUNE_SCRIPT" > "$IDLS_DIR/$name.codama.json"
}

fetch_and_prune "system" \
  "https://raw.githubusercontent.com/solana-program/system/main/program/idl.json"
fetch_and_prune "compute-budget" \
  "https://raw.githubusercontent.com/solana-program/compute-budget/main/program/idl.json"
fetch_and_prune "associated-token-account" \
  "https://raw.githubusercontent.com/solana-program/associated-token-account/main/pinocchio/interface/idl.json"
fetch_and_prune "token" \
  "https://raw.githubusercontent.com/solana-program/token/main/program/idl.json"

echo "Updated built-in IDLs in $IDLS_DIR"
