# heartwood-ledger

A Heartwood Nostr signer built as a Ledger embedded app (`no_std`, sideload on
Nano S+/X). It receives an encrypted request over chunked APDUs, does NIP-44
decrypt, NIP-46 dispatch and kind:24133 envelope signing on-device, and returns
the signed result over chunked APDUs. Key material derives from the Ledger's
own seed at `m/44'/1237'/727'/0'/0'` and never leaves the device. Shares its
NIP-44/NIP-46/derivation logic with the ESP32/ESP8266 firmware via the
`heartwood-common` crate (path dependency on a sibling `heartwood-esp32`
checkout).

## Build & Test

| Command | Purpose |
|---------|---------|
| `./scripts/e2e.sh` | Build for Nano S+, boot Speculos, run the host interop driver: the gate for any change |
| `docker run --rm -v "$(dirname "$PWD")":/ws -w /ws/heartwood-ledger ghcr.io/ledgerhq/ledger-app-builder/ledger-app-builder:latest cargo ledger build nanosplus` | Build only |
| `cd host && cargo run --release` | Run the host driver against an already-running Speculos |

Build and e2e need Docker (the Ledger toolchain and Speculos run in
containers) and a sibling `heartwood-esp32` checkout, since
`heartwood-common` is a path dependency.

## Structure

```
src/            App source (no_std): main.rs (APDU dispatch), sign_path.rs
                (NIP-44/46 dispatch loop), identity.rs, seed.rs, crypto.rs,
                approvals.rs (TOFU approval state), ui.rs (NBGL screens)
host/           Host interop driver (cargo crate): exercises the app over
                Speculos's TCP APDU port
scripts/e2e.sh  Build + Speculos + host driver, in one script
icons/, glyphs/ App icon source and processed glyphs (build.rs decodes GIFs
                to PNG at build time)
build.rs        Decodes glyphs; do not add the `image` crate as a dependency
ledger_app.toml Ledger app manifest (device targets, SDK)
```

## Conventions

- British English in prose and doc comments.
- Private key material is zeroised after use (`zeroize`, `Secret<N>`); never
  left in plain arrays.
- No key material in logs or APDU responses, ever.
- Curve operations go through the OS's cx syscalls via `heartwood-common`'s
  `ledger-backend` feature, not app-RAM implementations.
- Commit using conventional commits (`type: description`); a server-side CI
  check forbids the banned commit identity in tracked content and commit
  authorship.

## Key Files

| File | Purpose |
|------|---------|
| `src/main.rs` | APDU instruction set (`GET_VERSION`, `GET_APP_NAME`, `GET_PUBKEY`, `PROCESS`, `GET_RESULT`) and request dispatch |
| `src/sign_path.rs` | NIP-44 decrypt, NIP-46 dispatch, re-encrypt, envelope signing |
| `Cargo.toml` | `[package.metadata.ledger]` declares the BIP-32 path (`44'/1237'`), curve and device icons |
| `host/src/main.rs` | Interop driver: derivation vector, NIP-46 round trips, TOFU approval walk |

## Common Pitfalls

- `heartwood-common` must resolve as `../heartwood-esp32/common`; clone
  `heartwood-esp32` as a sibling directory before building.
- The app has a 24 KB heap; `MAX_REQUEST` in `src/main.rs` bounds accumulated
  request size well below what the ESP32 build accepts.
- Sideload only (Nano S/S+); Nano X/Stax/Flex distribution needs Ledger's
  review and a paid third-party security audit.
- Do not add the `image` crate; `build.rs` decodes glyph GIFs with `gif` and
  `png` directly to keep the build lean.
