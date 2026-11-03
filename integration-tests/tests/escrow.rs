//! Sandbox tests: a local NEAR node (near-workspaces) running the escrow, a NEP-141 token with USDC's storage rules
//! and a stand-in for the NEAR Intents verifier. Attestations are signed here with k256 over an EIP-712 encoding written
//! with sha3, independent of the contract's own (which runs on the host's `keccak256` and `ecrecover`).
//!
//! Build the wasm first: `./scripts/build.sh` (the tests read target/near/*/*.wasm).

use k256::ecdsa::SigningKey;
use near_workspaces::network::{Sandbox, ValidatorKey};
use near_workspaces::result::ExecutionFinalResult;
use near_workspaces::types::{Gas, NearToken};
use near_workspaces::{Account, Contract, Worker};
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};

const USDC: u128 = 1_000_000;
const DAY: u64 = 86_400;
const STORAGE: NearToken = NearToken::from_yoctonear(1_250_000_000_000_000_000_000);
const SLUG: &str = "predict-fwa";
const CLAIM_GRACE: u64 = 3 * DAY;

fn keccak(b: &[u8]) -> [u8; 32] {
    Keccak256::digest(b).into()
}

fn word(v: u64) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[24..].copy_from_slice(&v.to_be_bytes());
    w
}

fn hex0x(b: &[u8]) -> String {
    format!("0x{}", hex::encode(b))
}

/// The public test attester: keccak256("hey-proof-of-ship-near test attester"). Never HEY's key.
fn attester_key() -> SigningKey {
    SigningKey::from_slice(&keccak(b"hey-proof-of-ship-near test attester")).unwrap()
}

fn stranger_key() -> SigningKey {
    SigningKey::from_slice(&keccak(b"someone who is not the attester")).unwrap()
}

fn address(key: &SigningKey) -> [u8; 20] {
    let pt = key.verifying_key().to_encoded_point(false);
    let h = keccak(&pt.as_bytes()[1..]);
    h[12..].try_into().unwrap()
}

/// The builder's EVM address. Nobody holds its key in these tests; it only has to receive.
const BUILDER: [u8; 20] = [0xb1; 20];

#[derive(Clone)]
struct Att {
    project: [u8; 32],
    builder: [u8; 20],
    since: u64,
    until: u64,
    ships: u32,
    evidence: [u8; 32],
    issued_at: u64,
}

impl Att {
    fn json(&self) -> Value {
        json!({
            "project": hex0x(&self.project), "builder": hex0x(&self.builder), "since": self.since,
            "until": self.until, "ships": self.ships, "evidence": hex0x(&self.evidence), "issuedAt": self.issued_at,
        })
    }

    fn struct_hash(&self) -> [u8; 32] {
        let mut b = keccak(
            b"Ships(bytes32 project,address builder,uint64 since,uint64 until,uint32 ships,bytes32 evidence,uint64 issuedAt)",
        )
        .to_vec();
        b.extend_from_slice(&self.project);
        let mut a = [0u8; 32];
        a[12..].copy_from_slice(&self.builder);
        b.extend_from_slice(&a);
        b.extend_from_slice(&word(self.since));
        b.extend_from_slice(&word(self.until));
        b.extend_from_slice(&word(u64::from(self.ships)));
        b.extend_from_slice(&self.evidence);
        b.extend_from_slice(&word(self.issued_at));
        keccak(&b)
    }
}

/// The NEAR domain: name, version, chainId, salt = keccak256(escrow account id).
fn near_domain(chain_id: u64, escrow: &str) -> [u8; 32] {
    let mut b = keccak(b"EIP712Domain(string name,string version,uint256 chainId,bytes32 salt)").to_vec();
    b.extend_from_slice(&keccak(b"HEY Proof of Ship"));
    b.extend_from_slice(&keccak(b"1"));
    b.extend_from_slice(&word(chain_id));
    b.extend_from_slice(&keccak(escrow.as_bytes()));
    keccak(&b)
}

/// The Robinhood Chain ShipEscrow's domain: chain 4663, its contract as verifyingContract.
fn robinhood_domain() -> [u8; 32] {
    let mut b = keccak(b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)").to_vec();
    b.extend_from_slice(&keccak(b"HEY Proof of Ship"));
    b.extend_from_slice(&keccak(b"1"));
    b.extend_from_slice(&word(4663));
    let mut vc = [0u8; 32];
    vc[12..].copy_from_slice(&hex::decode("3Da17a89F1d29aB96419269514C798a8dd6125f2").unwrap());
    b.extend_from_slice(&vc);
    keccak(&b)
}

fn sign(key: &SigningKey, domain: &[u8; 32], att: &Att) -> Vec<u8> {
    let mut b = vec![0x19, 0x01];
    b.extend_from_slice(domain);
    b.extend_from_slice(&att.struct_hash());
    let (sig, rec) = key.sign_prehash_recoverable(&keccak(&b)).unwrap();
    let mut out = sig.to_bytes().to_vec();
    out.push(rec.to_byte() + 27);
    out
}

/// s' = n - s with v flipped: the same key's malleable twin.
fn high_s(sig: &[u8]) -> Vec<u8> {
    const N: [u8; 32] = [
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe, 0xba, 0xae,
        0xdc, 0xe6, 0xaf, 0x48, 0xa0, 0x3b, 0xbf, 0xd2, 0x5e, 0x8c, 0xd0, 0x36, 0x41, 0x41,
    ];
    let mut out = sig[..32].to_vec();
    let mut s = [0u8; 32];
    let mut borrow = 0i16;
    for i in (0..32).rev() {
        let mut d = N[i] as i16 - sig[32 + i] as i16 - borrow;
        borrow = if d < 0 {
            d += 256;
            1
        } else {
            0
        };
        s[i] = d as u8;
    }
    out.extend_from_slice(&s);
    out.push(if sig[64] == 27 { 28 } else { 27 });
    out
}

struct Env {
    _node: near_sandbox::Sandbox,
    worker: Worker<Sandbox>,
    escrow: Contract,
    token: Contract,
    intents: Contract,
    alice: Account,
    relayer: Account,
}

async fn setup() -> anyhow::Result<Env> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/near");
    let read = |name: &str| {
        std::fs::read(dir.join(name).join(format!("{name}.wasm")))
            .unwrap_or_else(|_| panic!("build the contracts first: ./scripts/build.sh ({name}.wasm missing)"))
    };
    // A fast-forwarded sandbox block advances the clock by the average of the min and max block production delays.
    // Raising the max (a single validator never waits for it) makes one fast-forwarded block about 7.5 minutes, so
    // the refund test crosses a deadline plus the three-day grace in about 1,200 blocks instead of a million.
    let node = near_sandbox::Sandbox::start_sandbox_with_config(near_sandbox::SandboxConfig {
        additional_config: Some(json!({"consensus": {"max_block_production_delay": {"secs": 900, "nanos": 0}}})),
        additional_accounts: vec![near_sandbox::GenesisAccount::default_with_name("registrar".parse()?)],
        ..Default::default()
    })
    .await?;
    let worker = near_workspaces::sandbox()
        .rpc_addr(&node.rpc_addr)
        .validator_key(ValidatorKey::HomeDir(node.home_dir.path().to_path_buf()))
        .await?;
    let root = worker.root_account()?;
    let sub = |name: &'static str, near: u128| {
        let root = root.clone();
        async move {
            root.create_subaccount(name)
                .initial_balance(NearToken::from_near(near))
                .transact()
                .await?
                .into_result()
                .map_err(anyhow::Error::from)
        }
    };
    let alice = sub("alice", 20).await?;
    let relayer = sub("relayer", 20).await?;
    let token = sub("usdc", 20).await?.deploy(&read("mock_ft")).await?.into_result()?;
    token
        .call("new")
        .args_json(json!({"owner_id": alice.id(), "total_supply": (1_000_000 * USDC).to_string()}))
        .transact()
        .await?
        .into_result()?;
    let intents = sub("intents", 20)
        .await?
        .deploy(&read("mock_intents"))
        .await?
        .into_result()?;
    intents.call("new").transact().await?.into_result()?;
    let escrow = sub("escrow", 30)
        .await?
        .deploy(&read("ship_escrow"))
        .await?
        .into_result()?;
    escrow
        .call("new")
        .args_json(json!({
            "attester": hex0x(&address(&attester_key())),
            "token": token.id(),
            "chain_id": 397,
            "guardian": null,
            "intents": intents.id(),
        }))
        .transact()
        .await?
        .into_result()?;
    // The escrow and the Intents verifier hold token balances, so they register like any account.
    for who in [escrow.id(), intents.id()] {
        alice
            .call(token.id(), "storage_deposit")
            .args_json(json!({"account_id": who, "registration_only": true}))
            .deposit(STORAGE)
            .transact()
            .await?
            .into_result()?;
    }
    Ok(Env {
        _node: node,
        worker,
        escrow,
        token,
        intents,
        alice,
        relayer,
    })
}

impl Env {
    async fn now(&self) -> anyhow::Result<u64> {
        Ok(self.worker.view_block().await?.timestamp() / 1_000_000_000)
    }

    /// Alice opens a bounty for `predict-fwa` (3 ships, 14 days) and funds it with 100 USDC.
    async fn open_bounty(&self, payout: &str) -> anyhow::Result<(u32, u64, u64)> {
        let deadline = self.now().await? + 14 * DAY;
        let res = self
            .alice
            .call(self.escrow.id(), "create")
            .args_json(json!({
                "builder": hex0x(&BUILDER), "slug": SLUG, "min_ships": 3, "deadline": deadline, "payout": payout,
            }))
            .deposit(NearToken::from_millinear(100))
            .transact()
            .await?;
        let id: u32 = res.into_result()?.json()?;
        self.alice
            .call(self.token.id(), "ft_transfer_call")
            .args_json(json!({
                "receiver_id": self.escrow.id(), "amount": (100 * USDC).to_string(),
                "msg": json!({"action": "fund", "id": id}).to_string(),
            }))
            .deposit(NearToken::from_yoctonear(1))
            .gas(Gas::from_tgas(100))
            .transact()
            .await?
            .into_result()?;
        let b = self.bounty(id).await?;
        assert_eq!(b["total"], (100 * USDC).to_string());
        Ok((id, b["start"].as_u64().unwrap(), deadline))
    }

    async fn bounty(&self, id: u32) -> anyhow::Result<Value> {
        Ok(self
            .escrow
            .view("get_bounty")
            .args_json(json!({"id": id}))
            .await?
            .json()?)
    }

    async fn ft_balance(&self, account: &str) -> anyhow::Result<u128> {
        let v: String = self
            .token
            .view("ft_balance_of")
            .args_json(json!({"account_id": account}))
            .await?
            .json()?;
        Ok(v.parse()?)
    }

    /// HEY's attestation for the bounty: `ships` counted from its start until a minute ago.
    async fn attestation(&self, start: u64, ships: u32) -> anyhow::Result<Att> {
        let now = self.now().await?;
        Ok(Att {
            project: keccak(SLUG.as_bytes()),
            builder: BUILDER,
            since: start,
            until: now,
            ships,
            evidence: keccak(b"ship:1,ship:2,ship:3"),
            issued_at: now,
        })
    }

    async fn claim(&self, id: u32, att: &Att, sig: &[u8]) -> anyhow::Result<ExecutionFinalResult> {
        Ok(self
            .relayer
            .call(self.escrow.id(), "claim")
            .args_json(json!({"id": id, "attestation": att.json(), "signature": hex0x(sig)}))
            .deposit(STORAGE)
            .gas(Gas::from_tgas(200))
            .transact()
            .await?)
    }

    async fn fast_forward_past(&self, t: u64) -> anyhow::Result<()> {
        // Sandbox blocks advance time by about a second each; jump in large steps until the chain clock passes t.
        while self.now().await? <= t {
            // About 450 seconds a block; overshooting by a block or two is fine.
            let gap = (t - self.now().await?) / 400 + 1;
            self.worker.fast_forward(gap.min(5_000)).await?;
        }
        Ok(())
    }
}

fn assert_refused(res: &ExecutionFinalResult, code: &str) {
    assert!(res.is_failure(), "expected {code}, but the call succeeded");
    let text = format!("{:?}", res.failures());
    assert!(text.contains(code), "expected {code}, got {text}");
}

/// The bool the escrow's last callback answered (true: paid or refunded), with the failures printed if the call failed.
fn settled(res: ExecutionFinalResult) -> bool {
    let failures = format!("{:?}", res.failures());
    res.json::<bool>()
        .unwrap_or_else(|e| panic!("no answer ({e}): {failures}"))
}

fn builder_account() -> String {
    hex0x(&BUILDER)
}

#[tokio::test]
async fn claim_pays_the_builders_eth_implicit_account_and_refuses_bad_claims() -> anyhow::Result<()> {
    let env = setup().await?;
    let (id, start, _) = env.open_bounty("near").await?;
    let domain = near_domain(397, env.escrow.id().as_str());

    // The view answers the same digest this test signs.
    let att = env.attestation(start, 3).await?;
    let digest: String = env
        .escrow
        .view("digest")
        .args_json(json!({"attestation": att.json()}))
        .await?
        .json()?;
    let mut pre = vec![0x19, 0x01];
    pre.extend_from_slice(&domain);
    pre.extend_from_slice(&att.struct_hash());
    assert_eq!(digest, hex0x(&keccak(&pre)));

    // Refused: below target.
    let low = env.attestation(start, 2).await?;
    assert_refused(
        &env.claim(id, &low, &sign(&attester_key(), &domain, &low)).await?,
        "ERR_NOT_ENOUGH_SHIPS",
    );
    // Refused: signed by someone other than the attester.
    assert_refused(
        &env.claim(id, &att, &sign(&stranger_key(), &domain, &att)).await?,
        "ERR_WRONG_SIGNER",
    );
    // Refused: the attester's signature for the Robinhood Chain escrow's domain.
    assert_refused(
        &env.claim(id, &att, &sign(&attester_key(), &robinhood_domain(), &att))
            .await?,
        "ERR_WRONG_SIGNER",
    );
    // Refused: the attester's signature for NEAR testnet (398) or for another escrow account.
    assert_refused(
        &env.claim(
            id,
            &att,
            &sign(&attester_key(), &near_domain(398, env.escrow.id().as_str()), &att),
        )
        .await?,
        "ERR_WRONG_SIGNER",
    );
    assert_refused(
        &env.claim(
            id,
            &att,
            &sign(&attester_key(), &near_domain(397, "another-escrow.near"), &att),
        )
        .await?,
        "ERR_WRONG_SIGNER",
    );
    // Refused: the malleable high-s twin of a valid signature.
    let good = sign(&attester_key(), &domain, &att);
    assert_refused(&env.claim(id, &att, &high_s(&good)).await?, "ERR_HIGH_S");
    // Refused: a count raised after signing.
    let mut raised = att.clone();
    raised.ships = 30;
    assert_refused(&env.claim(id, &raised, &good).await?, "ERR_WRONG_SIGNER");
    assert_eq!(env.bounty(id).await?["status"], "open");
    assert_eq!(env.ft_balance(&builder_account()).await?, 0);

    // A valid claim, submitted by a relayer, pays the builder's eth-implicit account (which does not even exist yet).
    let res = env.claim(id, &att, &good).await?;
    assert!(res.is_success(), "{:?}", res.failures());
    let logs = res.logs().join("\n");
    assert!(settled(res), "payout settled");
    assert_eq!(env.ft_balance(&builder_account()).await?, 100 * USDC);
    assert_eq!(env.ft_balance(env.escrow.id().as_str()).await?, 0);
    let b = env.bounty(id).await?;
    assert_eq!(
        (b["status"].as_str(), b["paid_out"].as_str()),
        (Some("paid"), Some("100000000"))
    );
    assert_eq!(b["builder_account"], builder_account());
    assert!(logs.contains("\"event\":\"claimed\""), "{logs}");
    assert!(
        logs.contains("\"builder_registered\":false"),
        "first payout registers the builder: {logs}"
    );

    // Refused: the same attestation again.
    assert_refused(&env.claim(id, &att, &good).await?, "ERR_ALREADY_CLAIMED");
    assert_eq!(env.ft_balance(&builder_account()).await?, 100 * USDC);
    // And no refund of a paid bounty, ever.
    let r = env
        .alice
        .call(env.escrow.id(), "refund")
        .args_json(json!({"id": id}))
        .gas(Gas::from_tgas(60))
        .transact()
        .await?;
    assert_refused(&r, "ERR_ALREADY_CLAIMED");

    // A second bounty to the same builder: already registered, so the claimer's storage deposit comes back.
    let (id2, start2, _) = env.open_bounty("near").await?;
    let att2 = env.attestation(start2, 5).await?;
    let res = env.claim(id2, &att2, &sign(&attester_key(), &domain, &att2)).await?;
    let logs = res.logs().join("\n");
    assert!(settled(res));
    assert_eq!(env.ft_balance(&builder_account()).await?, 200 * USDC);
    assert!(
        logs.contains(&format!(
            "\"builder_registered\":true,\"deposit_returned\":\"{}\"",
            STORAGE.as_yoctonear()
        )),
        "the whole storage deposit goes back to the claimer: {logs}"
    );
    Ok(())
}

#[tokio::test]
async fn intents_payout_credits_the_builder_and_falls_back_to_direct() -> anyhow::Result<()> {
    let env = setup().await?;
    let domain = near_domain(397, env.escrow.id().as_str());
    let key = format!("nep141:{}", env.token.id());

    // Paid into NEAR Intents for the account 0x… that the builder's EVM key controls.
    let (id, start, _) = env.open_bounty("intents").await?;
    let att = env.attestation(start, 3).await?;
    let res = env.claim(id, &att, &sign(&attester_key(), &domain, &att)).await?;
    assert!(settled(res));
    let held: String = env
        .intents
        .view("mt_balance_of")
        .args_json(json!({"account_id": builder_account(), "token_id": key}))
        .await?
        .json()?;
    assert_eq!(held, (100 * USDC).to_string());
    assert_eq!(env.bounty(id).await?["status"], "paid");

    // Intents refuses the deposit: the tokens come back, the bounty stays owed to the builder.
    env.intents
        .call("set_refuse")
        .args_json(json!({"refuse": true}))
        .transact()
        .await?
        .into_result()?;
    let (id2, start2, _) = env.open_bounty("intents").await?;
    let att2 = env.attestation(start2, 4).await?;
    let res = env.claim(id2, &att2, &sign(&attester_key(), &domain, &att2)).await?;
    assert!(!settled(res));
    let b = env.bounty(id2).await?;
    assert_eq!(
        (b["status"].as_str(), b["failed_payouts"].as_u64()),
        (Some("verified"), Some(1))
    );
    assert_eq!(env.ft_balance(env.escrow.id().as_str()).await?, 100 * USDC);
    // Anyone pays the same builder directly instead.
    let res = env
        .relayer
        .call(env.escrow.id(), "release")
        .args_json(json!({"id": id2, "direct": true}))
        .deposit(STORAGE)
        .gas(Gas::from_tgas(200))
        .transact()
        .await?;
    assert!(settled(res));
    assert_eq!(env.ft_balance(&builder_account()).await?, 100 * USDC);
    assert_eq!(env.bounty(id2).await?["status"], "paid");
    Ok(())
}

#[tokio::test]
async fn expired_claim_is_refused_and_funders_refund() -> anyhow::Result<()> {
    let env = setup().await?;
    let domain = near_domain(397, env.escrow.id().as_str());
    let (id, start, deadline) = env.open_bounty("near").await?;
    // A second funder.
    env.relayer
        .call(env.token.id(), "storage_deposit")
        .args_json(json!({"account_id": env.relayer.id(), "registration_only": true}))
        .deposit(STORAGE)
        .transact()
        .await?
        .into_result()?;
    env.alice
        .call(env.token.id(), "ft_transfer")
        .args_json(json!({"receiver_id": env.relayer.id(), "amount": (10 * USDC).to_string()}))
        .deposit(NearToken::from_yoctonear(1))
        .transact()
        .await?
        .into_result()?;
    env.relayer
        .call(env.token.id(), "ft_transfer_call")
        .args_json(json!({
            "receiver_id": env.escrow.id(), "amount": (10 * USDC).to_string(),
            "msg": json!({"action": "fund", "id": id}).to_string(),
        }))
        .deposit(NearToken::from_yoctonear(1))
        .gas(Gas::from_tgas(100))
        .transact()
        .await?
        .into_result()?;
    let alice_before = env.ft_balance(env.alice.id().as_str()).await?;

    // Refused: refunds before the deadline plus the grace.
    let r = env
        .alice
        .call(env.escrow.id(), "refund")
        .args_json(json!({"id": id}))
        .gas(Gas::from_tgas(60))
        .transact()
        .await?;
    assert_refused(&r, "ERR_REFUND_NOT_OPEN");

    // Past the deadline and the three-day grace.
    env.fast_forward_past(deadline + CLAIM_GRACE).await?;

    // Refused: a claim, even with a fresh, valid signature over a window ending at the deadline.
    let now = env.now().await?;
    let att = Att {
        until: deadline,
        issued_at: now,
        ..env.attestation(start, 9).await?
    };
    assert_refused(
        &env.claim(id, &att, &sign(&attester_key(), &domain, &att)).await?,
        "ERR_CLAIM_CLOSED",
    );

    // Each funder takes back exactly what they put in.
    let r = env
        .alice
        .call(env.escrow.id(), "refund")
        .args_json(json!({"id": id}))
        .gas(Gas::from_tgas(60))
        .transact()
        .await?;
    assert!(settled(r));
    assert_eq!(
        env.ft_balance(env.alice.id().as_str()).await?,
        alice_before + 100 * USDC
    );
    let relayer_before = env.ft_balance(env.relayer.id().as_str()).await?;
    let r = env
        .relayer
        .call(env.escrow.id(), "refund")
        .args_json(json!({"id": id}))
        .gas(Gas::from_tgas(60))
        .transact()
        .await?;
    assert!(settled(r));
    assert_eq!(
        env.ft_balance(env.relayer.id().as_str()).await?,
        relayer_before + 10 * USDC
    );
    // Refused: a second refund.
    let r = env
        .alice
        .call(env.escrow.id(), "refund")
        .args_json(json!({"id": id}))
        .gas(Gas::from_tgas(60))
        .transact()
        .await?;
    assert_refused(&r, "ERR_NOTHING_TO_REFUND");
    assert_eq!(env.ft_balance(env.escrow.id().as_str()).await?, 0);
    assert_eq!(env.bounty(id).await?["total"], "0");
    Ok(())
}
