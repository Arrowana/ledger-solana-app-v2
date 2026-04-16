# Ledger Squads App

## Status

Working end to end:
- save multisig
- list saved multisigs
- proposal vote: `approve` / `reject`
- Speculos web workflow

In progress:
- proposal create upgrade
- proposal execute upgrade
- device and host still disagree on the wrapped upgrade message hash

## Setup

```sh
bun install --cwd cli
```

## Common Commands

```sh
bun run check
bun run test
bun run build:ledger
bun run speculos:web
bun run test:speculos
```

Speculos web UI:

```text
http://127.0.0.1:5001
```

## CLI

Examples:

```sh
bun run --cwd cli src/cli.ts save-multisig \
  --transport hid \
  --rpc-url https://api.mainnet-beta.solana.com \
  --multisig <MULTISIG> \
  --derivation-path "m/44'/501'/0'/0'"

bun run --cwd cli src/cli.ts list-saved --transport hid

bun run --cwd cli src/cli.ts proposal-vote \
  --transport hid \
  --rpc-url https://api.mainnet-beta.solana.com \
  --multisig <MULTISIG> \
  --transaction-index 42 \
  --vote approve \
  --send
```

Speculos test env:

```sh
export LEDGER_SQUADS_TRANSPORT=speculos
export SPECULOS_HOST=127.0.0.1
export SPECULOS_APDU_PORT=9999
export LEDGER_SQUADS_NON_CONFIRM=1
```

## Security Model

The device API is intentionally narrow. The host does not send arbitrary fully-built transactions for the core multisig flows. It sends a small set of validated inputs, and the Ledger app constructs the final Solana message on device.

For proposal vote, the dynamic inputs are kept close to the minimum:
- multisig
- transaction index
- vote choice
- recent blockhash
- optional fee payer when allowed by the flow

Everything else is fixed or derived on device:
- program ID
- account order
- instruction layout
- proposal PDA
- signer binding

That matters for review. The less structure the host controls, the less room there is for a malicious client to smuggle in extra accounts, reorder metas, alter instruction data, or present a misleading host-side summary. The device review is based on the message it built itself, not on a host-assembled transaction blob.

Saved multisigs add a second control layer. A multisig is loaded onto the device first and bound to a specific Ledger derivation path. Later signing flows only proceed if the requested multisig matches a saved entry and the derived member key matches that entry. That gives the device stable local context before it signs:
- which multisig is being acted on
- which Ledger key is allowed to act for it
- whether the request is consistent with a previously approved binding

This is the main design constraint in the repo: minimize host-controlled transaction surface, maximize device-side derivation and verification, and keep review tied to a small set of explicit fields.
