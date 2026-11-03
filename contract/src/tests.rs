//! Unit tests: the EIP-712 encoding against viem's vectors, and every rule `claim`, `create`, `fund`, `refund` and
//! `retire` enforce. The cross-contract payout itself runs in the sandbox tests (integration-tests/).

use super::*;
use k256::ecdsa::SigningKey;
use near_sdk::test_utils::VMContextBuilder;
use near_sdk::{testing_env, PromiseResult as PR};

const ESCROW: &str = "ship-escrow.hey.near";
const TOKEN: &str = "usdc.test.near";
const ALICE: &str = "alice.near";
const BOB: &str = "bob.near";
const GUARDIAN: &str = "guardian.near";
const BUILDER: &str = "0x1111111111111111111111111111111111111111";
const SLUG: &str = "predict-fwa";
const T0: u64 = 1_762_300_000;
const DAY: u64 = 86_400;
const USDC: u128 = 1_000_000;

fn vectors() -> Value {
    serde_json::from_str(include_str!("vectors.json")).unwrap()
}

fn acc(s: &str) -> AccountId {
    s.parse().unwrap()
}

fn test_key() -> SigningKey {
    let seed = eip712::keccak(b"hey-proof-of-ship-near test attester");
    SigningKey::from_slice(&seed).unwrap()
}

fn other_key() -> SigningKey {
    SigningKey::from_slice(&eip712::keccak(b"not the attester")).unwrap()
}

fn address_of(key: &SigningKey) -> [u8; 20] {
    let pt = key.verifying_key().to_encoded_point(false);
    let h = eip712::keccak(&pt.as_bytes()[1..]);
    let mut a = [0u8; 20];
    a.copy_from_slice(&h[12..]);
    a
}

/// `r || s || v` with v in {27, 28}, low-s, as viem and HEY produce.
fn sign(key: &SigningKey, digest: &[u8; 32]) -> Vec<u8> {
    let (sig, recid) = key.sign_prehash_recoverable(digest).unwrap();
    let mut out = sig.to_bytes().to_vec();
    out.push(recid.to_byte() + 27);
    out
}

const N: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe, 0xba, 0xae, 0xdc,
    0xe6, 0xaf, 0x48, 0xa0, 0x3b, 0xbf, 0xd2, 0x5e, 0x8c, 0xd0, 0x36, 0x41, 0x41,
];

/// The malleable twin of a signature: s' = n - s, v flipped. Recovers the same key, so only an explicit high-s check
/// refuses it.
fn high_s(sig: &[u8]) -> Vec<u8> {
    let mut s = [0u8; 32];
    s.copy_from_slice(&sig[32..64]);
    let mut out = [0u8; 32];
    let mut borrow = 0i16;
    for i in (0..32).rev() {
        let mut d = N[i] as i16 - s[i] as i16 - borrow;
        borrow = if d < 0 {
            d += 256;
            1
        } else {
            0
        };
        out[i] = d as u8;
    }
    let mut twin = sig[..32].to_vec();
    twin.extend_from_slice(&out);
    twin.push(if sig[64] == 27 { 28 } else { 27 });
    twin
}

fn ctx(predecessor: &str, now: u64, deposit: u128, gas_tgas: u64) {
    let mut b = VMContextBuilder::new();
    b.current_account_id(acc(ESCROW))
        .predecessor_account_id(acc(predecessor))
        .signer_account_id(acc(predecessor))
        .block_timestamp(now * 1_000_000_000)
        .attached_deposit(NearToken::from_yoctonear(deposit))
        .prepaid_gas(Gas::from_tgas(gas_tgas))
        .account_balance(NearToken::from_near(10));
    testing_env!(b.build());
}

fn ctx_with_results(now: u64, results: Vec<PR>) {
    let mut b = VMContextBuilder::new();
    b.current_account_id(acc(ESCROW))
        .predecessor_account_id(acc(ESCROW))
        .block_timestamp(now * 1_000_000_000)
        .prepaid_gas(Gas::from_tgas(100))
        .account_balance(NearToken::from_near(10));
    testing_env!(
        b.build(),
        near_sdk::test_vm_config(),
        near_sdk::RuntimeFeesConfig::test(),
        Default::default(),
        results
    );
}

fn escrow_with(attester: &str, chain_id: u64, intents: Option<&str>) -> ShipEscrow {
    ctx(ESCROW, T0, 0, 100);
    ShipEscrow::new(
        attester.to_string(),
        acc(TOKEN),
        chain_id,
        Some(acc(GUARDIAN)),
        intents.map(acc),
        None,
    )
}

fn attester_hex() -> String {
    eip712::to_hex(&address_of(&test_key()))
}

/// An escrow with bounty 0 for `predict-fwa`, 3 ships by T0 + 14 days, funded 100 USDC by Alice.
fn funded(payout: Payout) -> ShipEscrow {
    let mut c = escrow_with(&attester_hex(), 397, Some("intents.near"));
    ctx(ALICE, T0, 10_000_000_000_000_000_000_000, 100);
    let id = c.create(BUILDER.into(), SLUG.into(), 3, T0 + 14 * DAY, Some(payout));
    assert_eq!(id, 0);
    ctx(TOKEN, T0 + 10, 0, 100);
    c.ft_on_transfer(acc(ALICE), U128(100 * USDC), r#"{"action":"fund","id":0}"#.into())
        .detach();
    c
}

fn att(ships: u32, since: u64, until: u64, issued_at: u64) -> Attestation {
    Attestation {
        project: eip712::to_hex(&eip712::keccak(SLUG.as_bytes())),
        builder: BUILDER.into(),
        since,
        until,
        ships,
        evidence: eip712::to_hex(&eip712::keccak(b"ship:a,ship:b,ship:c")),
        issued_at,
    }
}

fn signed(c: &ShipEscrow, key: &SigningKey, a: &Attestation) -> String {
    let d = eip712::digest(&c.domain_separator, &ShipEscrow::parse_attestation(a));
    eip712::to_hex(&sign(key, &d))
}

// ------------------------------------------------------------------ the encoding, against viem

#[test]
fn domain_and_digest_match_viem() {
    let v = vectors();
    assert_eq!(attester_hex(), v["attester"].as_str().unwrap().to_lowercase());
    let c = escrow_with(&attester_hex(), 397, None);
    let cfg = c.get_config();
    assert_eq!(cfg.salt, v["salt"].as_str().unwrap());
    assert_eq!(cfg.domain_separator, v["domainSeparator"]["397"].as_str().unwrap());
    let a: Attestation = serde_json::from_value(v["attestation"].clone()).unwrap();
    assert_eq!(c.digest(a.clone()), v["mainnet"]["digest"].as_str().unwrap());
    let sig = v["mainnet"]["signature"].as_str().unwrap().to_string();
    let r = c.recover_signer(a.clone(), sig);
    assert_eq!((r.signer, r.is_attester), (Some(attester_hex()), true));
    // A signature made by k256 here is the same signature viem made (RFC 6979 is deterministic).
    assert_eq!(signed(&c, &test_key(), &a), v["mainnet"]["signature"].as_str().unwrap());
}

#[test]
fn testnet_domain_matches_viem() {
    let v = vectors();
    let c = escrow_with(&attester_hex(), 398, None);
    let a: Attestation = serde_json::from_value(v["attestation"].clone()).unwrap();
    assert_eq!(c.digest(a.clone()), v["testnet"]["digest"].as_str().unwrap());
    assert!(
        c.recover_signer(a, v["testnet"]["signature"].as_str().unwrap().into())
            .is_attester
    );
}

#[test]
fn other_domains_recover_someone_else() {
    let v = vectors();
    let c = escrow_with(&attester_hex(), 397, None);
    let a: Attestation = serde_json::from_value(v["attestation"].clone()).unwrap();
    for case in ["testnet", "otherEscrow", "robinhood"] {
        let got = c.recover_signer(a.clone(), v[case]["signature"].as_str().unwrap().into());
        assert!(!got.is_attester, "{case} signature must not verify on this escrow");
        assert_ne!(got.signer, Some(attester_hex()));
    }
}

#[test]
fn attestation_accepts_hey_json_with_strings_or_numbers() {
    let a: Attestation = serde_json::from_str(
        r#"{"project":"0x9e806d6ac1bfb6b5f2adec1970138f021d8603335b0e94c3687f50b8e0786f2e","builder":"0x1111111111111111111111111111111111111111","since":"1762300000","until":1762900000,"ships":"3","evidence":"0x5a08edab336c91e0fc51fd44a6524018fbd1e66e507610c550322b511d6c56e1","issuedAt":"1762900060"}"#,
    )
    .unwrap();
    assert_eq!(
        (a.since, a.until, a.ships, a.issued_at),
        (1762300000, 1762900000, 3, 1762900060)
    );
}

#[test]
fn slug_rule() {
    assert!(is_hey_slug("predict-fwa"));
    assert!(is_hey_slug("0x-robinhood"));
    assert!(!is_hey_slug(""));
    assert!(!is_hey_slug("-lead"));
    assert!(!is_hey_slug("Predict-FWA"));
    assert!(!is_hey_slug("with space"));
    assert!(!is_hey_slug(&"a".repeat(129)));
    assert!(is_hey_slug(&"a".repeat(128)));
}

// ------------------------------------------------------------------ a valid claim

#[test]
fn valid_claim_verifies_and_starts_payout() {
    let mut c = funded(Payout::Near);
    let a = att(3, T0, T0 + 2 * DAY, T0 + 2 * DAY + 60);
    let sig = signed(&c, &test_key(), &a);
    ctx(BOB, T0 + 2 * DAY + 120, DEFAULT_TOKEN_STORAGE_DEPOSIT, 150);
    let _ = c.claim(0, a, sig);
    let b = c.get_bounty(0).unwrap();
    assert_eq!(b.status, Status::Paying);
    assert_eq!(b.ships, 3);
    assert_eq!(b.builder_account.as_str(), BUILDER);
}

#[test]
fn successful_direct_payout_marks_paid() {
    let mut c = funded(Payout::Near);
    let a = att(4, T0, T0 + DAY, T0 + DAY);
    let sig = signed(&c, &test_key(), &a);
    ctx(BOB, T0 + DAY + 1, DEFAULT_TOKEN_STORAGE_DEPOSIT, 150);
    let _ = c.claim(0, a, sig);
    ctx_with_results(T0 + DAY + 2, vec![PR::Successful(vec![])]);
    assert!(c.on_direct_payout(0, U128(100 * USDC)));
    let b = c.get_bounty(0).unwrap();
    assert_eq!((b.status, b.paid_out.0), (Status::Paid, 100 * USDC));
}

#[test]
fn failed_payout_stays_owed_and_can_be_released_again() {
    let mut c = funded(Payout::Intents);
    let a = att(3, T0, T0 + DAY, T0 + DAY);
    let sig = signed(&c, &test_key(), &a);
    ctx(BOB, T0 + DAY + 1, 0, 200);
    let _ = c.claim(0, a, sig);
    // Intents refunded everything (answered 0 used).
    ctx_with_results(T0 + DAY + 2, vec![PR::Successful(b"\"0\"".to_vec())]);
    assert!(!c.on_intents_payout(0, U128(100 * USDC)));
    let b = c.get_bounty(0).unwrap();
    assert_eq!((b.status, b.failed_payouts, b.paid_out.0), (Status::Verified, 1, 0));
    // Long after the claim window, the builder is still owed: release works, refund does not.
    ctx(ALICE, T0 + 60 * DAY, 0, 200);
    let _ = c.release(0, None);
    assert_eq!(c.get_bounty(0).unwrap().status, Status::Paying);
    ctx_with_results(T0 + 60 * DAY, vec![PR::Failed]);
    c.on_intents_payout(0, U128(100 * USDC));
    // After a failed Intents deposit, anyone may pay the same builder directly instead.
    ctx(BOB, T0 + 60 * DAY, DEFAULT_TOKEN_STORAGE_DEPOSIT, 150);
    let _ = c.release(0, Some(true));
    ctx_with_results(T0 + 60 * DAY, vec![PR::Successful(vec![])]);
    assert!(c.on_direct_payout(0, U128(100 * USDC)));
    assert_eq!(c.get_bounty(0).unwrap().status, Status::Paid);
}

#[test]
fn partial_intents_deposit_keeps_the_rest_owed() {
    let mut c = funded(Payout::Intents);
    let a = att(3, T0, T0 + DAY, T0 + DAY);
    let sig = signed(&c, &test_key(), &a);
    ctx(BOB, T0 + DAY + 1, 0, 200);
    let _ = c.claim(0, a, sig);
    ctx_with_results(
        T0 + DAY + 2,
        vec![PR::Successful(format!("\"{}\"", 40 * USDC).into_bytes())],
    );
    assert!(!c.on_intents_payout(0, U128(100 * USDC)));
    let b = c.get_bounty(0).unwrap();
    assert_eq!((b.status, b.paid_out.0), (Status::Verified, 40 * USDC));
}

#[test]
#[should_panic(expected = "ERR_NOT_VERIFIED")]
fn release_needs_a_verified_bounty() {
    let mut c = funded(Payout::Near);
    ctx(BOB, T0 + DAY, DEFAULT_TOKEN_STORAGE_DEPOSIT, 150);
    let _ = c.release(0, None);
}

#[test]
#[should_panic(expected = "ERR_DIRECT_NOT_ALLOWED")]
fn direct_fallback_needs_a_failed_intents_deposit() {
    let mut c = funded(Payout::Intents);
    let a = att(3, T0, T0 + DAY, T0 + DAY);
    let sig = signed(&c, &test_key(), &a);
    ctx(BOB, T0 + DAY + 1, 0, 200);
    let _ = c.claim(0, a, sig);
    ctx_with_results(
        T0 + DAY + 2,
        vec![PR::Successful(format!("\"{}\"", 40 * USDC).into_bytes())],
    );
    c.on_intents_payout(0, U128(100 * USDC)); // partial, not failed
    ctx(BOB, T0 + DAY + 3, DEFAULT_TOKEN_STORAGE_DEPOSIT, 150);
    let _ = c.release(0, Some(true));
}

// ------------------------------------------------------------------ refused claims

fn claim_with(c: &mut ShipEscrow, a: Attestation, sig: String, at: u64) {
    ctx(BOB, at, DEFAULT_TOKEN_STORAGE_DEPOSIT, 150);
    let _ = c.claim(0, a, sig);
}

#[test]
#[should_panic(expected = "ERR_ALREADY_CLAIMED")]
fn replayed_claim_is_refused() {
    let mut c = funded(Payout::Near);
    let a = att(3, T0, T0 + DAY, T0 + DAY);
    let sig = signed(&c, &test_key(), &a);
    claim_with(&mut c, a.clone(), sig.clone(), T0 + DAY + 1);
    claim_with(&mut c, a, sig, T0 + DAY + 2);
}

#[test]
#[should_panic(expected = "ERR_ALREADY_CLAIMED")]
fn replay_after_payout_is_refused() {
    let mut c = funded(Payout::Near);
    let a = att(3, T0, T0 + DAY, T0 + DAY);
    let sig = signed(&c, &test_key(), &a);
    claim_with(&mut c, a.clone(), sig.clone(), T0 + DAY + 1);
    ctx_with_results(T0 + DAY + 2, vec![PR::Successful(vec![])]);
    c.on_direct_payout(0, U128(100 * USDC));
    claim_with(&mut c, a, sig, T0 + DAY + 3);
}

#[test]
#[should_panic(expected = "ERR_HIGH_S")]
fn high_s_signature_is_refused() {
    let mut c = funded(Payout::Near);
    let a = att(3, T0, T0 + DAY, T0 + DAY);
    let d = eip712::digest(&c.domain_separator, &ShipEscrow::parse_attestation(&a));
    let twin = high_s(&sign(&test_key(), &d));
    claim_with(&mut c, a, eip712::to_hex(&twin), T0 + DAY + 1);
}

#[test]
fn high_s_twin_would_recover_the_attester_without_the_check() {
    // Proof the high-s test above is meaningful: the twin is a real signature by the same key.
    let c = funded(Payout::Near);
    let a = att(3, T0, T0 + DAY, T0 + DAY);
    let d = eip712::digest(&c.domain_separator, &ShipEscrow::parse_attestation(&a));
    let twin = high_s(&sign(&test_key(), &d));
    let v = twin[64] - 27;
    let pk = env::ecrecover(&d, &twin[..64], v, false).expect("the twin recovers without the malleability check");
    let h = eip712::keccak(&pk);
    assert_eq!(&h[12..], &address_of(&test_key()));
}

#[test]
#[should_panic(expected = "ERR_WRONG_SIGNER")]
fn robinhood_chain_domain_is_refused() {
    let mut c = funded(Payout::Near);
    let a = att(3, T0, T0 + DAY, T0 + DAY);
    // The same message signed for the Robinhood Chain escrow (chain 4663, verifyingContract) by the same key.
    let rh_domain = {
        let ty = eip712::keccak(b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)");
        let mut buf = ty.to_vec();
        buf.extend_from_slice(&eip712::keccak(b"HEY Proof of Ship"));
        buf.extend_from_slice(&eip712::keccak(b"1"));
        let mut chain = [0u8; 32];
        chain[24..].copy_from_slice(&4663u64.to_be_bytes());
        buf.extend_from_slice(&chain);
        let mut vc = [0u8; 32];
        vc[12..].copy_from_slice(&eip712::parse_hex::<20>("0x3Da17a89F1d29aB96419269514C798a8dd6125f2").unwrap());
        buf.extend_from_slice(&vc);
        eip712::keccak(&buf)
    };
    let d = eip712::digest(&rh_domain, &ShipEscrow::parse_attestation(&a));
    claim_with(&mut c, a, eip712::to_hex(&sign(&test_key(), &d)), T0 + DAY + 1);
}

#[test]
#[should_panic(expected = "ERR_WRONG_SIGNER")]
fn testnet_signature_is_refused_on_mainnet() {
    let mut c = funded(Payout::Near);
    let a = att(3, T0, T0 + DAY, T0 + DAY);
    let salt = eip712::salt_for(ESCROW);
    let d = eip712::digest(
        &eip712::domain_separator(398, &salt),
        &ShipEscrow::parse_attestation(&a),
    );
    claim_with(&mut c, a, eip712::to_hex(&sign(&test_key(), &d)), T0 + DAY + 1);
}

#[test]
#[should_panic(expected = "ERR_WRONG_SIGNER")]
fn another_escrows_signature_is_refused() {
    let mut c = funded(Payout::Near);
    let a = att(3, T0, T0 + DAY, T0 + DAY);
    let salt = eip712::salt_for("other-escrow.near");
    let d = eip712::digest(
        &eip712::domain_separator(397, &salt),
        &ShipEscrow::parse_attestation(&a),
    );
    claim_with(&mut c, a, eip712::to_hex(&sign(&test_key(), &d)), T0 + DAY + 1);
}

#[test]
#[should_panic(expected = "ERR_WRONG_SIGNER")]
fn wrong_attester_is_refused() {
    let mut c = funded(Payout::Near);
    let a = att(3, T0, T0 + DAY, T0 + DAY);
    let sig = signed(&c, &other_key(), &a);
    claim_with(&mut c, a, sig, T0 + DAY + 1);
}

#[test]
#[should_panic(expected = "ERR_WRONG_SIGNER")]
fn tampered_count_is_refused() {
    let mut c = funded(Payout::Near);
    let a = att(1, T0, T0 + DAY, T0 + DAY);
    let sig = signed(&c, &test_key(), &a);
    let mut more = a.clone();
    more.ships = 9;
    claim_with(&mut c, more, sig, T0 + DAY + 1);
}

#[test]
#[should_panic(expected = "ERR_NOT_ENOUGH_SHIPS")]
fn below_target_is_refused() {
    let mut c = funded(Payout::Near);
    let a = att(2, T0, T0 + DAY, T0 + DAY);
    let sig = signed(&c, &test_key(), &a);
    claim_with(&mut c, a, sig, T0 + DAY + 1);
}

#[test]
#[should_panic(expected = "ERR_CLAIM_CLOSED")]
fn claim_after_deadline_and_grace_is_refused() {
    let mut c = funded(Payout::Near);
    let end = T0 + 14 * DAY + CLAIM_GRACE + 1;
    let a = att(3, T0, T0 + 14 * DAY, end - 10);
    let sig = signed(&c, &test_key(), &a);
    claim_with(&mut c, a, sig, end);
}

#[test]
fn claim_inside_grace_is_accepted() {
    let mut c = funded(Payout::Near);
    let at = T0 + 14 * DAY + CLAIM_GRACE;
    let a = att(3, T0, T0 + 14 * DAY, at - 10);
    let sig = signed(&c, &test_key(), &a);
    claim_with(&mut c, a, sig, at);
    assert_eq!(c.get_bounty(0).unwrap().status, Status::Paying);
}

#[test]
#[should_panic(expected = "ERR_STALE_ATTESTATION")]
fn attestation_older_than_a_day_is_refused() {
    let mut c = funded(Payout::Near);
    let a = att(3, T0, T0 + DAY, T0 + DAY);
    let sig = signed(&c, &test_key(), &a);
    claim_with(&mut c, a, sig, T0 + 2 * DAY + 1);
}

#[test]
#[should_panic(expected = "ERR_STALE_ATTESTATION")]
fn attestation_from_the_future_is_refused() {
    let mut c = funded(Payout::Near);
    let a = att(3, T0, T0 + DAY, T0 + DAY + 100);
    let sig = signed(&c, &test_key(), &a);
    claim_with(&mut c, a, sig, T0 + DAY + 50);
}

#[test]
#[should_panic(expected = "ERR_WRONG_WINDOW")]
fn window_must_start_at_the_bounty_start() {
    let mut c = funded(Payout::Near);
    let a = att(3, T0 - 30 * DAY, T0 + DAY, T0 + DAY);
    let sig = signed(&c, &test_key(), &a);
    claim_with(&mut c, a, sig, T0 + DAY + 1);
}

#[test]
#[should_panic(expected = "ERR_WRONG_WINDOW")]
fn window_must_end_by_the_deadline() {
    let mut c = funded(Payout::Near);
    let a = att(3, T0, T0 + 15 * DAY, T0 + 15 * DAY);
    let sig = signed(&c, &test_key(), &a);
    claim_with(&mut c, a, sig, T0 + 15 * DAY + 1);
}

#[test]
#[should_panic(expected = "ERR_WRONG_WINDOW")]
fn window_must_end_by_issued_at() {
    let mut c = funded(Payout::Near);
    let a = att(3, T0, T0 + 2 * DAY, T0 + DAY);
    let sig = signed(&c, &test_key(), &a);
    claim_with(&mut c, a, sig, T0 + DAY + 1);
}

#[test]
#[should_panic(expected = "ERR_WRONG_BUILDER")]
fn another_builder_is_refused() {
    let mut c = funded(Payout::Near);
    let mut a = att(3, T0, T0 + DAY, T0 + DAY);
    a.builder = "0x2222222222222222222222222222222222222222".into();
    let sig = signed(&c, &test_key(), &a);
    claim_with(&mut c, a, sig, T0 + DAY + 1);
}

#[test]
#[should_panic(expected = "ERR_WRONG_PROJECT")]
fn another_project_is_refused() {
    let mut c = funded(Payout::Near);
    let mut a = att(3, T0, T0 + DAY, T0 + DAY);
    a.project = eip712::to_hex(&eip712::keccak(b"fly-ai"));
    let sig = signed(&c, &test_key(), &a);
    claim_with(&mut c, a, sig, T0 + DAY + 1);
}

#[test]
#[should_panic(expected = "ERR_NOTHING_FUNDED")]
fn unfunded_bounty_cannot_be_claimed() {
    let mut c = escrow_with(&attester_hex(), 397, None);
    ctx(ALICE, T0, 10_000_000_000_000_000_000_000, 100);
    c.create(BUILDER.into(), SLUG.into(), 3, T0 + 14 * DAY, None);
    let a = att(3, T0, T0 + DAY, T0 + DAY);
    let sig = signed(&c, &test_key(), &a);
    claim_with(&mut c, a, sig, T0 + DAY + 1);
}

#[test]
#[should_panic(expected = "ERR_STORAGE_DEPOSIT")]
fn direct_payout_needs_the_storage_deposit() {
    let mut c = funded(Payout::Near);
    let a = att(3, T0, T0 + DAY, T0 + DAY);
    let sig = signed(&c, &test_key(), &a);
    ctx(BOB, T0 + DAY + 1, 0, 150);
    let _ = c.claim(0, a, sig);
}

#[test]
#[should_panic(expected = "ERR_GAS")]
fn claim_needs_enough_gas() {
    let mut c = funded(Payout::Near);
    let a = att(3, T0, T0 + DAY, T0 + DAY);
    let sig = signed(&c, &test_key(), &a);
    ctx(BOB, T0 + DAY + 1, DEFAULT_TOKEN_STORAGE_DEPOSIT, 50);
    let _ = c.claim(0, a, sig);
}

// ------------------------------------------------------------------ create and fund

#[test]
#[should_panic(expected = "ERR_BAD_SLUG")]
fn create_refuses_a_slug_hey_never_signs() {
    let mut c = escrow_with(&attester_hex(), 397, None);
    ctx(ALICE, T0, 10_000_000_000_000_000_000_000, 100);
    c.create(BUILDER.into(), "Predict-FWA".into(), 3, T0 + 14 * DAY, None);
}

#[test]
#[should_panic(expected = "ERR_BAD_DEADLINE")]
fn create_refuses_a_deadline_under_a_day() {
    let mut c = escrow_with(&attester_hex(), 397, None);
    ctx(ALICE, T0, 10_000_000_000_000_000_000_000, 100);
    c.create(BUILDER.into(), SLUG.into(), 3, T0 + DAY - 1, None);
}

#[test]
#[should_panic(expected = "ERR_BAD_BUILDER")]
fn create_refuses_the_zero_address() {
    let mut c = escrow_with(&attester_hex(), 397, None);
    ctx(ALICE, T0, 10_000_000_000_000_000_000_000, 100);
    c.create(
        "0x0000000000000000000000000000000000000000".into(),
        SLUG.into(),
        3,
        T0 + 14 * DAY,
        None,
    );
}

#[test]
#[should_panic(expected = "ERR_NO_INTENTS")]
fn intents_payout_needs_an_intents_contract() {
    let mut c = escrow_with(&attester_hex(), 398, None);
    ctx(ALICE, T0, 10_000_000_000_000_000_000_000, 100);
    c.create(BUILDER.into(), SLUG.into(), 3, T0 + 14 * DAY, Some(Payout::Intents));
}

#[test]
#[should_panic(expected = "ERR_STORAGE")]
fn create_needs_its_storage_paid() {
    let mut c = escrow_with(&attester_hex(), 397, None);
    ctx(ALICE, T0, 0, 100);
    c.create(BUILDER.into(), SLUG.into(), 3, T0 + 14 * DAY, None);
}

#[test]
#[should_panic(expected = "ERR_WRONG_TOKEN")]
fn funding_with_another_token_is_refused() {
    let mut c = funded(Payout::Near);
    ctx("fake-usdc.near", T0 + 20, 0, 100);
    c.ft_on_transfer(acc(BOB), U128(USDC), r#"{"action":"fund","id":0}"#.into())
        .detach();
}

#[test]
#[should_panic(expected = "ERR_FUNDING_CLOSED")]
fn funding_after_the_deadline_is_refused() {
    let mut c = funded(Payout::Near);
    ctx(TOKEN, T0 + 14 * DAY, 0, 100);
    c.ft_on_transfer(acc(BOB), U128(USDC), r#"{"action":"fund","id":0}"#.into())
        .detach();
}

#[test]
#[should_panic(expected = "ERR_BAD_MSG")]
fn funding_with_a_bad_msg_is_refused() {
    let mut c = funded(Payout::Near);
    ctx(TOKEN, T0 + 20, 0, 100);
    c.ft_on_transfer(acc(BOB), U128(USDC), "0".into()).detach();
}

#[test]
fn funders_add_up() {
    let mut c = funded(Payout::Near);
    ctx(TOKEN, T0 + 20, 0, 100);
    c.ft_on_transfer(acc(BOB), U128(5 * USDC), r#"{"action":"fund","id":0}"#.into())
        .detach();
    c.ft_on_transfer(acc(ALICE), U128(USDC), r#"{"action":"fund","id":0}"#.into())
        .detach();
    let b = c.get_bounty(0).unwrap();
    assert_eq!((b.total.0, b.funders), (106 * USDC, 2));
    assert_eq!(c.get_contribution(0, acc(ALICE)).0, 101 * USDC);
}

// ------------------------------------------------------------------ refunds and retirement

#[test]
#[should_panic(expected = "ERR_REFUND_NOT_OPEN")]
fn refund_before_deadline_and_grace_is_refused() {
    let mut c = funded(Payout::Near);
    ctx(ALICE, T0 + 14 * DAY + CLAIM_GRACE, 0, 100);
    let _ = c.refund(0);
}

#[test]
fn refund_after_deadline_and_grace_returns_each_contribution() {
    let mut c = funded(Payout::Near);
    ctx(ALICE, T0 + 14 * DAY + CLAIM_GRACE + 1, 0, 100);
    let _ = c.refund(0);
    let b = c.get_bounty(0).unwrap();
    assert_eq!((b.total.0, b.funders), (0, 0));
    assert_eq!(c.get_contribution(0, acc(ALICE)).0, 0);
    // A failed transfer puts the contribution back.
    ctx_with_results(T0 + 20 * DAY, vec![PR::Failed]);
    assert!(!c.on_refund(0, acc(ALICE), U128(100 * USDC)));
    assert_eq!(c.get_contribution(0, acc(ALICE)).0, 100 * USDC);
    assert_eq!(c.get_bounty(0).unwrap().total.0, 100 * USDC);
}

#[test]
#[should_panic(expected = "ERR_NOTHING_TO_REFUND")]
fn refund_twice_is_refused() {
    let mut c = funded(Payout::Near);
    ctx(ALICE, T0 + 18 * DAY, 0, 100);
    let _ = c.refund(0);
    let _ = c.refund(0);
}

#[test]
#[should_panic(expected = "ERR_ALREADY_CLAIMED")]
fn verified_bounty_is_never_refunded() {
    let mut c = funded(Payout::Near);
    let a = att(3, T0, T0 + DAY, T0 + DAY);
    let sig = signed(&c, &test_key(), &a);
    claim_with(&mut c, a, sig, T0 + DAY + 1);
    ctx(ALICE, T0 + 30 * DAY, 0, 100);
    let _ = c.refund(0);
}

#[test]
#[should_panic(expected = "ERR_NOT_GUARDIAN")]
fn only_the_guardian_retires() {
    let mut c = funded(Payout::Near);
    ctx(ALICE, T0 + DAY, 1, 100);
    c.retire();
}

#[test]
fn retirement_stops_claims_and_opens_refunds() {
    let mut c = funded(Payout::Near);
    ctx(GUARDIAN, T0 + DAY, 1, 100);
    c.retire();
    assert!(c.get_config().retired);
    ctx(ALICE, T0 + DAY + 1, 0, 100);
    let _ = c.refund(0);
    assert_eq!(c.get_bounty(0).unwrap().total.0, 0);
}

#[test]
#[should_panic(expected = "ERR_RETIRED")]
fn retired_escrow_refuses_claims() {
    let mut c = funded(Payout::Near);
    ctx(GUARDIAN, T0 + DAY, 1, 100);
    c.retire();
    let a = att(3, T0, T0 + DAY, T0 + DAY);
    let sig = signed(&c, &test_key(), &a);
    claim_with(&mut c, a, sig, T0 + DAY + 1);
}

#[test]
#[should_panic(expected = "ERR_CHAIN_ID")]
fn init_refuses_a_non_near_chain_id() {
    escrow_with(&attester_hex(), 4663, None);
}
