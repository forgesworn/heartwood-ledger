#!/usr/bin/env bash
# Build the app for Nano S+, boot it in Speculos seeded with the canonical
# all-zero phrase, and run the host interop driver against it.
set -euo pipefail

cd "$(dirname "$0")/.."
WS_DIR="$(dirname "$PWD")"   # parent mount so ../heartwood-esp32/common resolves
BUILDER="ghcr.io/ledgerhq/ledger-app-builder/ledger-app-builder:latest"
SPECULOS="ghcr.io/ledgerhq/speculos:latest"
SEED="abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
ELF="target/nanosplus/release/heartwood-ledger"

## Reuse the host's crate cache (populated by `cargo fetch`) so the container
## downloads nothing — this network drops large transfers. The builder image
## keeps CARGO_HOME at /opt/.cargo.
CACHE="-v $HOME/.cargo/registry:/opt/.cargo/registry"

echo "== build =="
docker run --rm -v "$WS_DIR":/ws -w /ws/heartwood-ledger $CACHE "$BUILDER" \
  cargo ledger build nanosplus

echo "== speculos up =="
docker rm -f heartwood-speculos >/dev/null 2>&1 || true
docker run --rm -d --name heartwood-speculos \
  -v "$WS_DIR":/ws -p 9999:9999 -p 5001:5000 "$SPECULOS" \
  --model nanosp --display headless --apdu-port 9999 \
  --seed "$SEED" "/ws/heartwood-ledger/$ELF"
trap 'docker rm -f heartwood-speculos >/dev/null 2>&1 || true' EXIT

# Wait for the APDU port to accept connections.
for _ in $(seq 1 30); do
  if nc -z 127.0.0.1 9999 2>/dev/null; then break; fi
  sleep 1
done

echo "== host driver =="
# 5001 on the host: macOS AirPlay squats 5000.
(cd host && SPECULOS_API=127.0.0.1:5001 cargo run --release)
