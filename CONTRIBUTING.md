# Contributing to heartwood-ledger

## Setup

Prerequisites: Docker (the Ledger toolchain and Speculos run in containers),
Rust stable for the host driver.

```bash
git clone https://github.com/forgesworn/heartwood-ledger
git clone https://github.com/forgesworn/heartwood-esp32   # sibling dir — path dependency
cd heartwood-ledger
./scripts/e2e.sh
```

`scripts/e2e.sh` is the whole loop: builds the app for Nano S+ in
`ledger-app-builder`, boots it in Speculos seeded with the canonical all-zero
phrase, and runs the host interop driver against it. Green means derivation,
signing, NIP-44/46 and the approval policy all still match the rest of the
Heartwood ecosystem.

## Development Commands

| Command | Purpose |
|---------|---------|
| `./scripts/e2e.sh` | Build + full Speculos proof (the gate for any change) |
| `docker run --rm -v "$(dirname "$PWD")":/ws -w /ws/heartwood-ledger ghcr.io/ledgerhq/ledger-app-builder/ledger-app-builder:latest cargo ledger build nanosplus` | Build only |
| `cd host && cargo run --release` | Host driver against an already-running Speculos |

## Making Changes

1. Create a branch: `feat/short-description` or `fix/short-description`
2. Make your changes; keep shared logic in `heartwood-common`
   (heartwood-esp32) rather than duplicating it here — that crate is compiled
   unmodified for ESP32, ESP8266 and Ledger targets alike
3. `./scripts/e2e.sh` must pass
4. Commit using conventional commits: `type: description`
5. Open a pull request against `main`

## Code Style

- British English in all prose and doc comments
- Private key material is zeroised after use (`zeroize`, `Secret<N>`) — never
  left in plain arrays
- No key material in logs or APDU responses — ever
- Curve operations go through the cx syscalls (`heartwood-common`'s
  `ledger-backend`), not app-RAM implementations

## Security Issues

Please do not file public GitHub issues for security vulnerabilities. See
`SECURITY.md` for the responsible disclosure process.
