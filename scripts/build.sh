#!/bin/sh
# Build the escrow and the two sandbox stand-ins to wasm with cargo-near (near-sdk 5.29 needs it: it sets cfg(near)).
# Output: target/near/<crate>/<crate>.wasm
set -eu
cd "$(dirname "$0")/.."
for dir in contract mocks/mock-ft mocks/mock-intents; do
  (cd "$dir" && cargo near build non-reproducible-wasm --no-abi --locked)
done
ls -l target/near/*/*.wasm
