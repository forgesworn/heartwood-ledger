#!/usr/bin/env bash
# Build the app for Nano S+, boot it in Speculos seeded with the canonical
# all-zero phrase, and run the host interop driver against it.
set -euo pipefail

cd "$(dirname "$0")/.."
APP_DIR="$PWD"
WS_DIR="$(dirname "$PWD")"   # parent mount so ../heartwood-esp32/common resolves
IMAGE="ghcr.io/ledgerhq/ledger-app-builder/ledger-app-dev-tools:latest"
SEED="abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
ELF="target/nanosplus/release/heartwood-ledger"

echo "== build =="
docker run --rm -v "$WS_DIR":/ws -w /ws/heartwood-ledger "$IMAGE" \
  cargo ledger build nanosplus

echo "== speculos up =="
docker rm -f heartwood-speculos >/dev/null 2>&1 || true
docker run --rm -d --name heartwood-speculos \
  -v "$WS_DIR":/ws -w /ws/heartwood-ledger -p 9999:9999 "$IMAGE" \
  speculos --model nanosp --display headless --apdu-port 9999 \
    --seed "$SEED" "$ELF"
trap 'docker rm -f heartwood-speculos >/dev/null 2>&1 || true' EXIT

# Wait for the APDU port to accept connections.
for _ in $(seq 1 30); do
  if nc -z 127.0.0.1 9999 2>/dev/null; then break; fi
  sleep 1
done

echo "== host driver =="
(cd host && cargo run --release)
