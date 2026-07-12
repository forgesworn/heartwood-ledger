# heartwood-ledger

[![CI](https://github.com/forgesworn/heartwood-ledger/actions/workflows/ci.yml/badge.svg)](https://github.com/forgesworn/heartwood-ledger/actions/workflows/ci.yml)
[![GitHub Sponsors](https://img.shields.io/github/sponsors/TheCryptoDonkey?logo=githubsponsors&color=ea4aaa&label=Sponsor)](https://github.com/sponsors/TheCryptoDonkey)

> **Working prototype — emulator-proven, not yet bench-tested on hardware.** Part of the
> [ForgeSworn identity stack](https://github.com/forgesworn) — the
> [Heartwood](https://github.com/forgesworn/heartwood) hardware Nostr signer, running as a
> [Ledger](https://developers.ledger.com/) embedded app instead of an ESP32/ESP8266.

The Ledger takes the place of the ESP behind `heartwood-bridge`: the host delivers the body of an
`ENCRYPTED_REQUEST` (serial frame `0x10`) over chunked APDUs, and the app does everything on-device —
NIP-44 decrypt → NIP-46 dispatch → re-encrypt → sign the kind:24133 envelope — then returns the
`SIGN_ENVELOPE_RESPONSE` (`0x35`) JSON. The host never sees plaintext or key material.

## Why this works

- **Same identity.** The master key is the BIP-32 node at `m/44'/1237'/727'/0'/0'` — the exact path
  the ESP firmware, provision CLI and sapwood derive from a BIP-39 phrase. Restore a heartwood
  phrase through Ledger's own onboarding and you get the identical npub and identical
  [nsec-tree](https://github.com/forgesworn/nsec-tree) personas. The host driver proves this against
  the frozen all-zero vector.
- **Same code.** The NIP-44, NIP-46, derivation and identity logic is
  [`heartwood-common`](https://github.com/forgesworn/heartwood-esp32) — the firmware's `no_std`
  crate — compiled unmodified for the Ledger target. `src/sign_path.rs` is the ESP8266 dispatch
  loop with the OLED/button seams removed.
- **Keys on hardware.** Derivation happens through `os_perso_derive_node_bip32`; the OS restricts
  this app to the `44'/1237'` subtree and the raw seed is never accessible.

## Build (Docker)

```sh
docker run --rm -v "$(dirname "$PWD")":/ws -w /ws/heartwood-ledger \
  ghcr.io/ledgerhq/ledger-app-builder/ledger-app-dev-tools:latest \
  cargo ledger build nanosplus
```

(The parent directory is mounted so the `heartwood-common` path dependency resolves.)

## Prove it (Speculos)

Seed the emulator with the canonical all-zero test phrase, then run the host driver:

```sh
docker run --rm -v "$(dirname "$PWD")":/ws -w /ws/heartwood-ledger -p 9999:9999 \
  ghcr.io/ledgerhq/ledger-app-builder/ledger-app-dev-tools:latest \
  speculos --model nanosp --display headless --apdu-port 9999 \
    --seed "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about" \
    target/nanosplus/release/heartwood-ledger

cd host && cargo run
```

The driver asserts, in order: derivation matches the frozen vector (identity interop), a full
NIP-46 `get_public_key` round trip (envelope BIP-340 signature + NIP-44 verified), `sign_event` as
master **gated by the on-device TOFU approval** (the driver walks Speculos's buttons to Approve),
persona derive/switch/sign with **no buttons pressed** (proving the TOFU grant), and an unknown
client's signing request **rejected on-device** (signed denial envelope).

## Signing policy

The Heartwood TOFU model, on Ledger buttons: non-signing methods (`get_public_key`, NIP-44/04
encrypt/decrypt) are the connect-safe tier and never prompt. The **first `sign_event` from an
unknown client** blocks on an Approve/Reject choice screen showing the client npub prefix and the
event kind; approval stores the client in app NVM and grants unattended signing thereafter — the
bunker model. Every curve operation runs on the OS's cx syscalls (the taproot path), not in
app RAM.

## Using it with the bridge

`heartwood-bridge` (branch `ledger-transport`) speaks this app's APDU protocol directly:

```sh
HEARTWOOD_TRANSPORT=ledger-tcp HEARTWOOD_SERIAL_PORT=127.0.0.1:9999 heartwood-bridge
```

No `bridge.secret` — a Ledger authenticates its user (PIN) and gates signing on-device.

## Remaining caveats

- **Not yet run on a physical Nano S+** — sideload + bench test is the next gate. The bridge's USB
  HID transport (`ledger-hid`, hand-rolled hidraw) is framing-tested but likewise bench-gated.
- **24 KB heap** bounds the maximum event size well below the ESP32's.
- **Sideload only** (Nano S/S+). Distribution to Nano X/Stax/Flex requires Ledger's review and a
  paid third-party security audit.

All curve operations — pubkey derivation, BIP-340 signing, NIP-44 ECDH (including the even-y lift
of the peer key) — run through the OS's cx syscalls via `heartwood-common`'s `ledger-backend`
feature (branch `ledger-backend` in heartwood-esp32); `k256` is no longer in the app at all.

## Licence

MIT.
