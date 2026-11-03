# Security policy

## Reporting a vulnerability

Please report privately through GitHub's "Report a vulnerability" (Security, then Advisories) on this repository, or
email hi@heyresearch.xyz with "security" in the subject. Do not open a public issue. We aim to acknowledge within 3
working days. There is no bug bounty for this repository.

## What the escrow trusts

- **No owner, no admin method.** The contract has no method that moves tokens except a payout to the builder HEY's
  signature names (`claim`, `release`) and each funder's own refund (`refund`). The attester, token, chain id, guardian
  and Intents verifier are fixed at init. On NEAR an account's full-access key can redeploy its code, so a mainnet
  escrow is only immutable once its operator deletes those keys after deployment (README, "Deploy"). Until then, trust
  the operator as well.
- **The attester.** One EVM address, fixed at init, signs the ship counts and the payout address. HEY signs only for a
  payout address its evidence ties to the project, so a bounty opened with someone else's address is never claimable
  and its funders refund after the window. A compromised attester key can release a bounty early to the builder its
  creator named; it cannot redirect funds or touch refunds of other bounties.
- **Signatures** are EIP-712 with domain `EIP712Domain(string name,string version,uint256 chainId,bytes32 salt)`,
  chain id 397 (mainnet) or 398 (testnet) and salt `keccak256(escrow account id)`, so a signature for the Robinhood
  Chain escrow, for the other NEAR network or for another escrow account never verifies. High-s signatures are refused
  before the host's `ecrecover` is asked, and the host is asked with its malleability check on. An attestation carries
  the bounty's exact start and payout address, a window ending by the deadline, and expires after one day. One
  attestation can release several bounties for the same project and payout address opened in the same second; that is
  intended (they asked for the same thing).
- **Token.** One NEP-141, fixed at init (USDC on NEAR on mainnet). `ft_on_transfer` refuses any other token and any
  message but a fund of an open bounty, so the token refunds the sender. Tokens sent with a plain `ft_transfer` cannot
  be recovered.
- **Payout state.** A claim marks the bounty `verified` before the cross-contract payout and `paid` only when the token
  confirms it; a failed payout leaves it `verified` (owed to the builder, retryable by anyone, never refundable). While a
  payout is in flight the bounty is `paying` and nothing else can touch it.
- **Storage.** A new bounty's storage is paid by its creator. A new funder's record (about 100 bytes) is paid from the
  escrow's own NEAR balance, so the operator keeps a few NEAR on the account; a funder costs an attacker more than that
  in token storage registration and gas. The builder's token registration at payout is paid by the claimer.
- **Guardian.** An optional NEAR account that can do one thing, once: retire the escrow (claims stop, refunds open at
  once). It can never move a token anywhere but back to its funders. A bounty already verified before retirement stays
  owed to its builder.

## Test keys

`keccak256("hey-proof-of-ship-near test attester")` is a public test key, used by the tests and trusted by the testnet
demo escrow. It is not HEY's key and no mainnet escrow trusts it.
