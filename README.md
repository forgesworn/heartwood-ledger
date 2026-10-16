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
  ghcr.io/ledgerhq/ledger-app-builder/ledger-app-builder:latest \
  cargo ledger build nanosplus
```

Clone [heartwood-esp32](https://github.com/forgesworn/heartwood-esp32) next to this checkout first;
the parent directory is mounted so the `heartwood-common` path dependency resolves. This is the image
`scripts/e2e.sh` builds with, and its tag is unpinned.

## Prove it (Speculos)

`./scripts/e2e.sh` runs the whole proof: build, boot Speculos, run the host driver, tidy up. To do
it by hand, build the app as above, then start Speculos in the background, seeded with the
canonical all-zero test phrase:

```sh
docker run --rm -d --name heartwood-speculos \
  -v "$(dirname "$PWD")":/ws -p 9999:9999 -p 5001:5000 \
  ghcr.io/ledgerhq/speculos:latest \
  --model nanosp --display headless --apdu-port 9999 \
  --seed "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about" \
  /ws/heartwood-ledger/target/nanosplus/release/heartwood-ledger
```

Wait a few seconds until port 9999 accepts connections (`e2e.sh` polls for up to 30), then run the
driver and stop the emulator:

```sh
(cd host && SPECULOS_API=127.0.0.1:5001 cargo run --release)
docker rm -f heartwood-speculos
```

The driver talks to Speculos on two ports, both overridable by environment variable:

- **APDUs** go over TCP to `SPECULOS_ADDR`, default `127.0.0.1:9999` (the `--apdu-port` above).
- **Approval prompts** are driven through Speculos's REST automation API at `SPECULOS_API`, default
  `127.0.0.1:5000`: it reads the current screen and presses the buttons.

Speculos serves the REST API on port 5000 inside the container. The command above publishes it as
5001 on the host because macOS AirPlay occupies 5000, hence the `SPECULOS_API` override. Elsewhere
you can publish `-p 5000:5000` and leave the variable unset.

The driver asserts, in order: derivation matches the frozen vector (identity interop), a full
NIP-46 `get_public_key` round trip (envelope BIP-340 signature + NIP-44 verified), `sign_event` as
master **gated by the on-device TOFU approval** (the driver walks Speculos's buttons via the REST
API to Approve), persona derive/switch/sign with **no buttons pressed** (proving the TOFU grant),
and an unknown client's signing request **rejected on-device** (signed denial envelope).

## Signing policy

The Heartwood TOFU model, on Ledger buttons: non-signing methods (`get_public_key`, NIP-44/04
encrypt/decrypt) are the connect-safe tier and never prompt. The **first `sign_event` from an
unknown client** blocks on an Approve/Reject choice screen showing the client npub prefix and the
event kind; approval stores the client in app NVM and grants unattended signing thereafter — the
bunker model. Every curve operation runs on the OS's cx syscalls (the taproot path), not in
app RAM.

## APDU protocol

Every command uses CLA `0xE0`. `src/main.rs` is the source of truth.

| INS | Name | P1 | P2 | Data in | Data out |
|-----|------|----|----|---------|----------|
| `0x03` | `GET_VERSION` | `0x00` | `0x00` | none | `[major, minor, patch]`, from the crate version |
| `0x04` | `GET_APP_NAME` | `0x00` | `0x00` | none | `Heartwood` (ASCII) |
| `0x05` | `GET_PUBKEY` | `0x00` | `0x00` | none | 32-byte x-only master pubkey |
| `0x10` | `PROCESS` | chunk index, from 0, strictly sequential | `0x80` more chunks follow, `0x00` last chunk | next slice of the request body (the host driver sends 250-byte chunks; the APDU data limit is 255) | empty, except on the last chunk: the result length as a big-endian `u16` |
| `0x11` | `GET_RESULT` | chunk index, from 0 | `0x00` | none | the next slice of the result: up to 250 bytes at offset `P1 * 250` |

The request body is `[master_pk 32][client_pk 32][created_at u64-be 8][NIP-44 ciphertext, base64 ASCII]`:
at least 72 bytes. The app enforces only an 8 KiB (8192 byte) total across all chunks, not a per-chunk size. The result is the
signed kind:24133 envelope, as UTF-8 JSON.

A round trip:

1. Send the body as `PROCESS` chunks `0, 1, 2, ...` with P2 `0x80`, and P2 `0x00` on the last.
   Chunk 0 discards any earlier partial request and any pending result.
2. The last chunk's reply carries the result length `L`. It can block: a `sign_event` from a client
   the device has not yet approved waits on the on-device Approve/Reject screen, so use a long read
   timeout (the host driver uses 60 seconds).
3. Send `GET_RESULT` with P1 `0, 1, 2, ...` until you hold `L` bytes. The end of the data is given by
   `L`, not by an error. The result is not consumed by reading and stays available until the next
   `PROCESS` chunk 0.

An on-device Reject is not a status word. `PROCESS` still returns `0x9000`, and the signed envelope
carries the NIP-46 error `-32000` `denied at device`. Other NIP-46 errors arrive the same way, for
example `-32601` `method not supported` and `-32602` for malformed `sign_event` params.

Status words:

| SW | Meaning |
|----|---------|
| `0x9000` | Success |
| `0x6A86` | Known INS with the wrong P1/P2 (for example a P2 other than `0x00`/`0x80` on `PROCESS`) |
| `0x6D00` | Unknown INS |
| `0x6F00` | Communication or response-buffer error |
| `0xB009` | Master key derivation failed (`GET_PUBKEY`, or the last `PROCESS` chunk) |
| `0xB010` | Accumulated request would exceed 8192 bytes; the buffer is discarded |
| `0xB011` | `PROCESS` chunk index was not the expected next one; the buffer is discarded |
| `0xB012` | Request could not be processed: shorter than 72 bytes, NIP-44 decrypt failure, unparsable request, or signing failure |
| `0xB013` | No result pending, or `GET_RESULT` chunk index past the end of the result |

`AppSW` also declares `0x6985` (`Deny`) and `WrongApduLength`; the app code never returns either.

Over Speculos's TCP APDU port each message is framed as a big-endian `u32` length followed by the
APDU (`CLA INS P1 P2 Lc data`). The reply is a big-endian `u32` data length, the data, then the
two-byte status word.

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
- **Sideload only.** Nano S+ is the only device the app is built for and proven on (`e2e.sh` and CI
  build `nanosplus`); `ledger_app.toml` also lists Nano X, which is never built or tested here.
  Distribution to Nano X/Stax/Flex requires Ledger's review and a paid third-party security audit.

All curve operations — pubkey derivation, BIP-340 signing, NIP-44 ECDH (including the even-y lift
of the peer key) — run through the OS's cx syscalls via `heartwood-common`'s `ledger-backend`
feature (merged to heartwood-esp32 `main`; no branch checkout needed); `k256` is no longer in the
app at all.

## Licence

MIT.
