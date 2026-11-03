#!/bin/sh
# The whole Proof of Ship flow on NEAR testnet, against the demo escrow, in about a minute:
#   open a bounty, fund it with test USDC, sign an attestation, see a below-target claim refused, claim, see the
#   builder's eth-implicit account paid, see the replay refused.
#
#   ./scripts/demo-testnet.sh <your-account>.testnet
#
# Needs near-cli-rs and node. If the account does not exist it is created from the testnet faucet (no human step),
# with its key in ~/.near-credentials (the "legacy keychain").
#
# The demo escrow trusts the PUBLIC test attester keccak256("hey-proof-of-ship-near test attester"), so anyone can
# sign its attestations with scripts/sign.mjs. A mainnet escrow trusts only HEY Research Lab's attester key.
set -eu
cd "$(dirname "$0")/.."

ME="${1:?usage: ./scripts/demo-testnet.sh <your-account>.testnet}"
ESCROW="${ESCROW:-escrow.proof-of-ship-hey.testnet}"
TOKEN="${TOKEN:-tusdc.proof-of-ship-hey.testnet}"
SLUG="${SLUG:-predict-fwa}"
SIGN="${SIGN:-sign-with-legacy-keychain}"
NET="network-config testnet"

[ -d scripts/node_modules/viem ] || npm ci --prefix scripts --no-audit --no-fund >/dev/null

view() { near contract call-function as-read-only "$1" "$2" json-args "$3" $NET now 2>/dev/null; }
# Prints the call's return value on stdout; the CLI's own report goes to stderr (kept in a log, shown on failure).
call() {
  log=$(mktemp)
  if near contract call-function as-transaction "$1" "$2" json-args "$3" prepaid-gas "$4" attached-deposit "$5" \
      sign-as "$ME" $NET "$SIGN" send 2>"$log"; then
    grep -E "Transaction ID" "$log" | sed 's/.*Transaction ID: /    tx: https:\/\/testnet.nearblocks.io\/txns\//' >&2 || true
    rm -f "$log"
  else
    grep -E "Transaction ID|ERR_[A-Z_]+[^\"]*|Smart contract panicked[^\"]*" -o "$log" | sed 's/^/    /' >&2
    rm -f "$log"
    return 1
  fi
}
js() { node -e "const v = JSON.parse(require('fs').readFileSync(0, 'utf8')); console.log($1)"; }

if ! near account view-account-summary "$ME" $NET now >/dev/null 2>&1; then
  echo "== creating $ME from the testnet faucet"
  near account create-account sponsor-by-faucet-service "$ME" autogenerate-new-keypair save-to-legacy-keychain $NET create >/dev/null 2>&1
fi

# A fresh builder address for this run: an EVM address whose NEAR eth-implicit account is 0x<lowercase hex>.
BUILDER="${BUILDER:-$(node scripts/address.mjs)}"
BUILDER_ACCOUNT=$(echo "$BUILDER" | tr 'A-F' 'a-f')
echo "== builder: $BUILDER  (NEAR account $BUILDER_ACCOUNT)"

echo "== 1. take 100 test USDC from the faucet"
call "$TOKEN" storage_deposit "{\"account_id\":\"$ME\",\"registration_only\":true}" '30.0 Tgas' '0.00125 NEAR' >/dev/null
call "$TOKEN" faucet '{"amount":"100000000"}' '30.0 Tgas' '0 NEAR' >/dev/null

echo "== 2. open a bounty: 3 ships for $SLUG within 7 days"
DEADLINE=$(( $(date +%s) + 7 * 86400 ))
ID=$(call "$ESCROW" create "{\"builder\":\"$BUILDER\",\"slug\":\"$SLUG\",\"min_ships\":3,\"deadline\":$DEADLINE,\"payout\":\"near\"}" '30.0 Tgas' '0.01 NEAR')
echo "    bounty id: $ID"

echo "== 3. fund it with 100 USDC (ft_transfer_call)"
call "$TOKEN" ft_transfer_call "{\"receiver_id\":\"$ESCROW\",\"amount\":\"100000000\",\"msg\":\"{\\\"action\\\":\\\"fund\\\",\\\"id\\\":$ID}\"}" '100.0 Tgas' '1 yoctoNEAR' >/dev/null
START=$(view "$ESCROW" get_bounty "{\"id\":$ID}" | js 'v.start')
echo "    bounty: $(view "$ESCROW" get_bounty "{\"id\":$ID}" | js '`${v.status}, ${v.total} units, start ${v.start}`')"

sleep 3
NOW=$(( $(date +%s) - 2 ))
attest() { node scripts/sign.mjs --escrow "$ESCROW" --chain 398 --slug "$SLUG" --builder "$BUILDER" --since "$START" --until "$NOW" --issued-at "$NOW" --ships "$1" 2>/dev/null; }

echo "== 4. a claim with 2 ships (below the target of 3) is refused"
LOW=$(attest 2)
ARGS=$(echo "$LOW" | js "JSON.stringify({ id: $ID, attestation: v.attestation, signature: v.signature })")
call "$ESCROW" claim "$ARGS" '150.0 Tgas' '0.00125 NEAR' >/dev/null && { echo "unexpected success" >&2; exit 1; } || true

echo "== 5. claim with HEY-style attestation of 3 ships (signed with the public test key)"
GOOD=$(attest 3)
echo "$GOOD" | js 'JSON.stringify(v.attestation)' | sed 's/^/    attestation: /'
ARGS=$(echo "$GOOD" | js "JSON.stringify({ id: $ID, attestation: v.attestation, signature: v.signature })")
call "$ESCROW" claim "$ARGS" '150.0 Tgas' '0.00125 NEAR' | sed 's/^/    paid: /'

echo "== 6. the builder's eth-implicit account holds the bounty"
echo "    ft_balance_of($BUILDER_ACCOUNT) = $(view "$TOKEN" ft_balance_of "{\"account_id\":\"$BUILDER_ACCOUNT\"}") (100 USDC at 6 decimals)"
echo "    bounty: $(view "$ESCROW" get_bounty "{\"id\":$ID}" | js '`${v.status}, paid_out ${v.paid_out}, ships ${v.ships}`')"

echo "== 7. the same attestation again is refused"
call "$ESCROW" claim "$ARGS" '150.0 Tgas' '0.00125 NEAR' >/dev/null && { echo "unexpected success" >&2; exit 1; } || true
echo "== done"
