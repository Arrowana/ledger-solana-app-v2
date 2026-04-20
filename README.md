# Solana v2

This is AI slop. Do not treat it as safe to use with real funds.

## Status

Current layout:

- `cli/`: Rust host CLI
- `ledger-app/`: Rust Ledger app based on `app-boilerplate-rust`
- `app-integration-tests/`: Rust integration test runner for repeatable local smoke tests

Current firmware surface:

- `app-config`
- `get-pubkey`
- `sign-message`

Built-in on-device instruction decoding:

- `system`
- `compute-budget`
- `associated-token-account`
- `token`

Current host-side tools:

- `inspect-message` decodes Solana message bytes against a Codama IDL
- `speculos-ui` reads the current screen and sends button presses

## Setup

```sh
cargo check -p ledger-solana-cli
```

## Common Commands

```sh
cargo check -p ledger-solana-cli
cargo test -p ledger-solana-cli
./scripts/build-ledger.sh
cargo run -p app-integration-tests -- speculos-smoke
SPECULOS_API_PORT=5001 bash ./scripts/run-speculos.sh
cargo run -p ledger-solana-cli --bin speculos-ui -- screen
```

Speculos web UI:

```text
http://127.0.0.1:5001
```

## CLI

Examples:

```sh
cargo run -p ledger-solana-cli -- app-config \
  --transport speculos \
  --speculos-port 9999

cargo run -p ledger-solana-cli -- get-pubkey \
  --transport speculos \
  --speculos-port 9999 \
  --derivation-path "m/44'/501'/0'/0'"

cargo run -p ledger-solana-cli -- sign-message \
  --transport speculos \
  --speculos-port 9999 \
  --derivation-path "m/44'/501'/0'/0'" \
  --message-hex <SOLANA_MESSAGE_HEX>

cargo run -p ledger-solana-cli -- inspect-message \
  --idl <PROGRAM.codama.json> \
  --program-id <PROGRAM_ID> \
  --message-hex <SOLANA_MESSAGE_HEX>
```

Speculos test env:

```sh
export LEDGER_SOLANA_TRANSPORT=speculos
export SPECULOS_HOST=127.0.0.1
export SPECULOS_APDU_PORT=9999
export LEDGER_SOLANA_APDU_TIMEOUT_MS=120000
```

## Smoke Test

Run the full Dockerized build + Speculos smoke flow with:

```sh
cargo run -p app-integration-tests -- speculos-smoke
```

That command performs the steps in order:

- builds the Ledger app with `scripts/build-ledger.sh`
- launches Speculos on free local ports
- checks `app-config`
- checks `get-pubkey`
- submits built-in decode smoke cases for `system`, `compute-budget`, `associated-token-account`, and `token`
- walks the review UI through the Speculos API, asserts the decoded screens, approves, and waits for the signature result

Useful variants:

```sh
cargo run -p app-integration-tests -- speculos-smoke --skip-build
cargo run -p app-integration-tests -- speculos-smoke --cases system-transfer
cargo run -p app-integration-tests -- speculos-smoke --cases compute-budget-limit --cases token-transfer
cargo run -p app-integration-tests -- speculos-smoke --skip-build --api-port 5001 --manual-review
```

`--manual-review` still sends the sign payload and waits for the review flow to start, but it stops driving the buttons. The runner prints the Speculos web UI URL and waits for you to finish the review manually in the browser.

## Security Model

The current app reviews and signs raw Solana message bytes provided by the host. For the bundled programs in `idls/`, the device also decodes instruction names and arguments on-device during review. For any other program, it falls back to the generic account/data review flow.

That means the main trust boundary today is:

- the host chooses the exact Solana message bytes
- the device parses those bytes, decodes bundled program instructions when available, and signs only after approval
- the host can still use `inspect-message` plus a Codama IDL to decode any program payload before or after signing

Current verified flow:

- run `cargo run -p app-integration-tests -- speculos-smoke`
