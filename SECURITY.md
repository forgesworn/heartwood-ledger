# Security Policy

## Reporting a Vulnerability

Please do **not** file public GitHub issues for security vulnerabilities.

Send a DM via Nostr to the ForgeSworn team. Our public key is listed at [github.com/forgesworn](https://github.com/forgesworn). Use NIP-44 encryption.

Alternatively, email the address in the ForgeSworn GitHub org profile.

We aim to acknowledge reports within 48 hours and provide a timeline within 7 days.

## Scope

In scope:
- The APDU protocol and its chunk reassembly (`src/main.rs`)
- The TOFU signing-approval policy and the NVM-persisted approved-client set
  (`src/approvals.rs`, `src/ui.rs`) — the gate between "host can deliver a
  request" and "device signs it"
- The signing path (`src/sign_path.rs`): NIP-44 decrypt → NIP-46 dispatch →
  re-encrypt → envelope signing, shared with the ESP signers via
  [`heartwood-common`](https://github.com/forgesworn/heartwood-esp32)
- The `ledger-backend` curve operations in `heartwood-common` (cx syscall
  usage, the even-y `lift_x`, key zeroisation)

Out of scope:
- Side-channel resistance beyond what BOLOS/the secure element provides
- Physical attacks on the Ledger device itself
- Speculos (the emulator is a development tool, not a deployment target)

## Cryptographic Primitives

| Function | Algorithm | Implementation |
|----------|-----------|----------------|
| Master key | BIP-32 node at `m/44'/1237'/727'/0'/0'` | `os_perso_derive_node_bip32` (OS) |
| Child key derivation | nsec-tree HMAC-SHA256 | `heartwood-common` (`hmac` + `sha2`) |
| Signing | BIP-340 Schnorr (secp256k1) | `cx_ecschnorr` (secure element) |
| NIP-44 ECDH | secp256k1, x-coordinate | `cx_ecdh` + OS modular maths (secure element) |
| NIP-44 cipher | ChaCha20 + HMAC-SHA256 | `heartwood-common` (RustCrypto) |
| Secret memory hygiene | Zeroise after use | `zeroize` + `Secret<N>` |

## Known Limitations

- **Not yet bench-tested on physical hardware** — all verification is against
  Speculos. Sideload to a Nano S+ is the next gate.
- Signing nonces are deterministic (zero aux data) by design, matching the
  radio-off ESP signers — the documented trade-off for needing no runtime RNG.
- The 24 KB app heap bounds the maximum event size.
- No independent security audit has been completed.

## Frozen Test Vectors

`scripts/e2e.sh` drives the app in Speculos seeded with the canonical
all-zero BIP-39 phrase and asserts derivation, signatures, NIP-44 round trips
and the approval policy against host-side k256 and the frozen vector shared
with the provision CLI, sapwood and the ESP firmware. Any change that breaks
it is a protocol-breaking change, not a refactor.
