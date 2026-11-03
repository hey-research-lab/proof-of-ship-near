#!/bin/sh
# Deploy and initialise the escrow in ONE transaction (init is private to the escrow account, so nobody can front-run
# it). Uses near-cli-rs (https://github.com/near/near-cli-rs). The escrow account must already exist and hold about
# 3 NEAR for the contract's storage.
#
#   Testnet:  NETWORK=testnet ESCROW=escrow.you.testnet ATTESTER=0x... TOKEN=<nep141> ./scripts/deploy.sh
#   Mainnet:  NETWORK=mainnet ESCROW=<account>.near ATTESTER=0x... GUARDIAN=<account>.near \
#             CONFIRM_MAINNET=yes ./scripts/deploy.sh
#
# Mainnet defaults: TOKEN = USDC on NEAR (17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1, Circle's
# native USDC, 6 decimals, storage 0.00125 NEAR) and INTENTS = intents.near (the NEAR Intents verifier).
# Testnet has no NEAR Intents, so INTENTS stays empty there and bounties pay eth-implicit accounts only.
#
# Mainnet deployment is the founder's, from the founder's own NEAR account. This script never runs it without
# CONFIRM_MAINNET=yes and prints the exact command first.
set -eu
cd "$(dirname "$0")/.."

NETWORK="${NETWORK:?NETWORK=testnet or mainnet}"
ESCROW="${ESCROW:?ESCROW=<the escrow account id>}"
ATTESTER="${ATTESTER:?ATTESTER=0x<HEY attester EVM address>}"
SIGN="${SIGN:-sign-with-keychain}"
WASM="${WASM:-target/near/ship_escrow/ship_escrow.wasm}"

case "$NETWORK" in
  mainnet)
    CHAIN_ID=397
    TOKEN="${TOKEN:-17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1}"
    INTENTS="${INTENTS:-intents.near}"
    ;;
  testnet)
    CHAIN_ID=398
    TOKEN="${TOKEN:?TOKEN=<a testnet NEP-141, e.g. Circle testnet USDC 3e2210e1184b45b64c8a434c0a7e7b23cc04ea7eb7a6c3c32520d03d4afcb8af>}"
    INTENTS="${INTENTS:-}"
    ;;
  *)
    echo "NETWORK must be testnet or mainnet" >&2
    exit 2
    ;;
esac

case "$ATTESTER" in
  0x[0-9a-fA-F]*) [ ${#ATTESTER} -eq 42 ] || { echo "ATTESTER must be 0x + 40 hex" >&2; exit 2; } ;;
  *) echo "ATTESTER must be 0x + 40 hex" >&2; exit 2 ;;
esac

json_or_null() { if [ -n "$1" ]; then printf '"%s"' "$1"; else printf 'null'; fi; }
ARGS=$(printf '{"attester":"%s","token":"%s","chain_id":%s,"guardian":%s,"intents":%s}' \
  "$ATTESTER" "$TOKEN" "$CHAIN_ID" "$(json_or_null "${GUARDIAN:-}")" "$(json_or_null "$INTENTS")")

[ -f "$WASM" ] || ./scripts/build.sh
echo "wasm:     $WASM ($(shasum -a 256 "$WASM" | cut -c1-64))"
echo "network:  $NETWORK (EIP-712 chainId $CHAIN_ID)"
echo "escrow:   $ESCROW"
echo "init:     $ARGS"
set -- near contract deploy "$ESCROW" use-file "$WASM" with-init-call new json-args "$ARGS" \
  prepaid-gas '100.0 Tgas' attached-deposit '0 NEAR' network-config "$NETWORK" "$SIGN" send
echo "command:  $*"

if [ "$NETWORK" = mainnet ] && [ "${CONFIRM_MAINNET:-}" != yes ]; then
  echo "Not sent: set CONFIRM_MAINNET=yes to deploy to mainnet." >&2
  exit 1
fi
"$@"

echo
echo "Check it:  near contract call-function as-read-only $ESCROW get_config json-args '{}' network-config $NETWORK now"
echo "The escrow needs a storage registration with its token before anyone can fund it:"
echo "  near contract call-function as-transaction $TOKEN storage_deposit json-args '{\"account_id\":\"$ESCROW\",\"registration_only\":true}' prepaid-gas '30.0 Tgas' attached-deposit '0.00125 NEAR' sign-as <you> network-config $NETWORK $SIGN send"
