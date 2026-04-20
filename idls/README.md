# Bundled IDLs

This folder centralizes the minified, pruned Codama IDLs that are embedded into the Ledger app for on-device instruction decoding.

Current bundled programs:

- `system.codama.json`
- `compute-budget.codama.json`
- `associated-token-account.codama.json`
- `token.codama.json`

Regenerate them from the official upstream `solana-program/*` repositories with:

```sh
./scripts/update-idls.sh
```

The sync script fetches the upstream IDLs, prunes non-essential nodes with `scripts/prune-codama.jq`, and writes minified JSON files back into this folder.
