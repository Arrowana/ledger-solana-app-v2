# Ledger Solana App V2

This is AI slop. Do not treat it as safe to use with real funds.

## Status

Current layout:

- `cli/`: Rust host CLI
- `ledger-app/`: Rust Ledger app based on `app-boilerplate-rust`

Current firmware surface:

- `app-config`
- `get-pubkey`
- `sign-message`

Current host-side tools:

- `inspect-message` decodes Solana message bytes against a Codama IDL
- `speculos-ui` reads the current screen and sends button presses
- legacy Squads-specific commands are still present in the CLI, but they target APDUs that are no longer exposed by the current firmware build

## Setup

```sh
cargo check -p ledger-squads-cli
```

## Common Commands

```sh
cargo check -p ledger-squads-cli
cargo test -p ledger-squads-cli
./scripts/build-ledger.sh
SPECULOS_API_PORT=5001 bash ./scripts/run-speculos.sh
cargo run -p ledger-squads-cli --bin speculos-ui -- screen
```

Speculos web UI:

```text
http://127.0.0.1:5001
```

## CLI

Examples:

```sh
cargo run -p ledger-squads-cli -- app-config \
  --transport speculos \
  --speculos-port 9999

cargo run -p ledger-squads-cli -- get-pubkey \
  --transport speculos \
  --speculos-port 9999 \
  --derivation-path "m/44'/501'/0'/0'"

cargo run -p ledger-squads-cli -- sign-message \
  --transport speculos \
  --speculos-port 9999 \
  --derivation-path "m/44'/501'/0'/0'" \
  --message-hex <SOLANA_MESSAGE_HEX>

cargo run -p ledger-squads-cli -- inspect-message \
  --idl squads-v4.codama.json \
  --message-hex <SOLANA_MESSAGE_HEX>
```

Speculos test env:

```sh
export LEDGER_SQUADS_TRANSPORT=speculos
export SPECULOS_HOST=127.0.0.1
export SPECULOS_APDU_PORT=9999
export LEDGER_SQUADS_APDU_TIMEOUT_MS=120000
```

## Security Model

The current app reviews and signs raw Solana message bytes provided by the host. It does not reconstruct Squads transactions on device in the current firmware build.

That means the main trust boundary today is:

- the host chooses the exact Solana message bytes
- the device parses those bytes, renders a generic review flow, and signs only after approval
- the host can use `inspect-message` plus a Codama IDL to decode instruction payloads before or after signing

Current verified flow:

- build the Ledger app with `./scripts/build-ledger.sh`
- run Speculos against the built ELF
- query `app-config`
- derive `get-pubkey`
- submit `sign-message` and approve the review screens in Speculos

Current gap:

- the old Squads-specific CLI flows have not yet been ported onto the generic `sign-message` firmware surface
