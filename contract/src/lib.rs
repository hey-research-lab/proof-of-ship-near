//! # Proof of Ship on NEAR
//!
//! A bounty escrow for one NEP-141 token (USDC on NEAR first). Anyone opens a bounty for a project with a page on
//! HEY Research Lab (`heyresearch.xyz/project/<slug>`), naming the builder's EVM payout address, a ship target and a
//! deadline; anyone funds it with `ft_transfer_call`. The tokens leave the escrow in only two ways:
//!
//! * **claim**: anyone submits HEY's EIP-712 attestation that it counted at least the target number of ships for the
//!   project between the bounty's start and a moment no later than its deadline. The signature is checked on chain
//!   with `env::ecrecover` and `env::keccak256` against the attester fixed at init. The tokens go to the builder named
//!   in the signature: its NEAR eth-implicit account (`0x…`, NEP-518), or, if the bounty asked for it, its account
//!   inside NEAR Intents (`intents.near`), which the same EVM key controls.
//! * **refund**: each funder takes back exactly what they put in once the deadline plus a three-day claim grace has
//!   passed without a claim, or at once if the guardian retired the escrow.
//!
//! There is no owner and no admin method. The attester, token, chain id and guardian are fixed at init; the guardian
//! can only retire the escrow (claims stop, refunds open).

pub mod eip712;

use eip712::{Ships, SigError};
use near_contract_standards::fungible_token::receiver::FungibleTokenReceiver;
use near_sdk::json_types::U128;
use near_sdk::serde::{Deserialize, Deserializer};
use near_sdk::serde_json::{self, json, Value};
use near_sdk::store::{LookupMap, Vector};
use near_sdk::{
    env, ext_contract, near, require, AccountId, BorshStorageKey, Gas, NearToken, PanicOnDefault, Promise,
    PromiseError, PromiseOrValue,
};

/// An attestation older than this cannot be used.
pub const MAX_ATTESTATION_AGE: u64 = 86_400;
/// After the deadline the builder still has this long to claim with ships made before it; refunds open when it ends.
pub const CLAIM_GRACE: u64 = 3 * 86_400;
pub const MIN_DURATION: u64 = 86_400;
pub const MAX_DURATION: u64 = 365 * 86_400;
/// The longest HEY slug (`^[a-z0-9][a-z0-9-]{0,127}$`).
pub const MAX_SLUG_LEN: usize = 128;
/// `storage_deposit` minimum of USDC on NEAR (and of every near-contract-standards token): 0.00125 NEAR.
pub const DEFAULT_TOKEN_STORAGE_DEPOSIT: u128 = 1_250_000_000_000_000_000_000;
const DEAD_ADDRESS: [u8; 20] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xde, 0xad];

const GAS_FT_TRANSFER: Gas = Gas::from_tgas(10);
const GAS_FT_TRANSFER_CALL: Gas = Gas::from_tgas(80);
const GAS_STORAGE_DEPOSIT: Gas = Gas::from_tgas(10);
const GAS_STORAGE_BALANCE_OF: Gas = Gas::from_tgas(5);
const GAS_ON_PAYOUT: Gas = Gas::from_tgas(15);
const GAS_ON_REFUND: Gas = Gas::from_tgas(15);
/// Gas the storage callback keeps for itself and the promises it spawns.
const GAS_ON_STORAGE_CHECKED: Gas = Gas::from_tgas(55);
/// A claim or release must carry at least this much prepaid gas, so it never reaches a half-done state for want of it.
pub const MIN_GAS_DIRECT: Gas = Gas::from_tgas(100);
pub const MIN_GAS_INTENTS: Gas = Gas::from_tgas(130);
const ONE_YOCTO: NearToken = NearToken::from_yoctonear(1);

/// Where a bounty pays its builder.
#[near(serializers = [borsh, json])]
#[serde(rename_all = "snake_case")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Payout {
    /// An NEP-141 transfer to the builder's eth-implicit account `0x…` (registered for storage first if needed).
    Near,
    /// An `ft_transfer_call` deposit into NEAR Intents (`intents.near`) for the account `0x…`, which the builder's EVM
    /// key controls with ERC-191 signatures; from there it withdraws to any chain Intents supports.
    Intents,
}

#[near(serializers = [borsh, json])]
#[serde(rename_all = "snake_case")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Fundable until the deadline, claimable until the deadline plus the grace, refundable after that.
    Open,
    /// HEY's attestation was accepted: the tokens are owed to the builder. A payout that failed is retried with
    /// `release`; a verified bounty is never refunded.
    Verified,
    /// A payout is in flight.
    Paying,
    /// Paid in full.
    Paid,
}

#[near(serializers = [borsh])]
pub struct Bounty {
    pub builder: [u8; 20],
    pub project: [u8; 32],
    pub slug: String,
    pub creator: AccountId,
    pub start: u64,
    pub deadline: u64,
    pub min_ships: u32,
    pub payout: Payout,
    pub status: Status,
    /// What funders put in and have not taken back.
    pub total: u128,
    /// What has reached the builder.
    pub paid_out: u128,
    pub funders: u32,
    /// From the accepted attestation.
    pub ships: u32,
    pub evidence: [u8; 32],
    pub failed_payouts: u32,
}

#[near(serializers = [json])]
pub struct BountyView {
    pub id: u32,
    pub slug: String,
    pub project: String,
    pub builder: String,
    /// The NEAR account the payout reaches: the builder's eth-implicit account id.
    pub builder_account: AccountId,
    pub creator: AccountId,
    pub start: u64,
    pub deadline: u64,
    pub refunds_open_at: u64,
    pub min_ships: u32,
    pub payout: Payout,
    pub status: Status,
    pub total: U128,
    pub paid_out: U128,
    pub funders: u32,
    pub ships: u32,
    pub evidence: Option<String>,
    pub failed_payouts: u32,
}

#[near(serializers = [json])]
pub struct ConfigView {
    pub attester: String,
    pub token: AccountId,
    pub chain_id: u64,
    pub salt: String,
    pub domain_separator: String,
    pub guardian: Option<AccountId>,
    pub intents: Option<AccountId>,
    pub token_storage_deposit: U128,
    pub retired: bool,
    pub max_attestation_age: u64,
    pub claim_grace: u64,
    pub min_duration: u64,
    pub max_duration: u64,
}

/// `recover_signer`'s answer: who signed, whether that is this escrow's attester, or why no key recovers.
#[near(serializers = [json])]
#[derive(Debug, PartialEq, Eq)]
pub struct SignerView {
    pub signer: Option<String>,
    pub is_attester: bool,
    pub error: Option<String>,
    pub digest: String,
}

/// HEY's attestation, exactly as `GET /api/v1/proof-of-ship/attestation` returns it under `attestation`. Seconds
/// may be JSON numbers or decimal strings.
#[near(serializers = [json])]
#[derive(Clone, Debug)]
pub struct Attestation {
    pub project: String,
    pub builder: String,
    #[serde(deserialize_with = "flex_u64")]
    pub since: u64,
    #[serde(deserialize_with = "flex_u64")]
    pub until: u64,
    #[serde(deserialize_with = "flex_u32")]
    pub ships: u32,
    pub evidence: String,
    #[serde(rename = "issuedAt", alias = "issued_at", deserialize_with = "flex_u64")]
    pub issued_at: u64,
}

#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde", untagged)]
enum NumOrStr {
    N(u64),
    S(String),
}

fn flex_u64<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    match NumOrStr::deserialize(d)? {
        NumOrStr::N(n) => Ok(n),
        NumOrStr::S(s) => s.parse::<u64>().map_err(near_sdk::serde::de::Error::custom),
    }
}

fn flex_u32<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let v = flex_u64(d)?;
    u32::try_from(v).map_err(near_sdk::serde::de::Error::custom)
}

/// What an `ft_transfer_call` to the escrow asks for.
#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde", tag = "action", rename_all = "snake_case")]
enum FtAction {
    Fund { id: u32 },
}

#[derive(BorshStorageKey)]
#[near]
enum StorageKey {
    Bounties,
    Contributed,
}

#[ext_contract(ext_ft)]
#[allow(dead_code)]
trait FungibleToken {
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>);
    fn ft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        amount: U128,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<U128>;
    fn storage_deposit(&mut self, account_id: Option<AccountId>, registration_only: Option<bool>) -> Value;
    fn storage_balance_of(&self, account_id: AccountId) -> Option<Value>;
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct ShipEscrow {
    attester: [u8; 20],
    token: AccountId,
    token_storage_deposit: u128,
    guardian: Option<AccountId>,
    intents: Option<AccountId>,
    chain_id: u64,
    salt: [u8; 32],
    domain_separator: [u8; 32],
    retired: bool,
    bounties: Vector<Bounty>,
    contributed: LookupMap<(u32, AccountId), u128>,
}

fn now_seconds() -> u64 {
    env::block_timestamp() / 1_000_000_000
}

/// The NEAR account an EVM address owns: `0x` + 40 lower-case hex characters (NEP-518 eth-implicit account, and the
/// account NEAR Intents derives for a secp256k1 key).
pub fn eth_implicit_account(address: &[u8; 20]) -> AccountId {
    eip712::to_hex(address)
        .parse()
        .unwrap_or_else(|_| env::panic_str("ERR_ACCOUNT_ID"))
}

/// A HEY slug: `^[a-z0-9][a-z0-9-]{0,127}$`. HEY signs only for a slug of that shape, so the escrow refuses any other.
pub fn is_hey_slug(slug: &str) -> bool {
    let b = slug.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_SLUG_LEN
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

fn emit(event: &str, data: Value) {
    env::log_str(&format!(
        "EVENT_JSON:{}",
        json!({"standard": "hey-proof-of-ship", "version": "1.0.0", "event": event, "data": [data]})
    ));
}

fn sig_error_message(e: SigError) -> &'static str {
    match e {
        SigError::Malformed => "ERR_BAD_SIGNATURE: not 65 bytes of 0x hex",
        SigError::BadV => "ERR_BAD_SIGNATURE: v must be 27 or 28",
        SigError::ZeroComponent => "ERR_BAD_SIGNATURE: r or s is zero",
        SigError::HighS => "ERR_HIGH_S: s is in the upper half of the curve order",
        SigError::Unrecoverable => "ERR_BAD_SIGNATURE: no key recovers from it",
    }
}

impl ShipEscrow {
    fn parse_attestation(att: &Attestation) -> Ships {
        Ships {
            project: eip712::parse_hex::<32>(&att.project)
                .unwrap_or_else(|| env::panic_str("ERR_BAD_ATTESTATION: project is not 0x bytes32")),
            builder: eip712::parse_hex::<20>(&att.builder)
                .unwrap_or_else(|| env::panic_str("ERR_BAD_ATTESTATION: builder is not a 0x address")),
            since: att.since,
            until: att.until,
            ships: att.ships,
            evidence: eip712::parse_hex::<32>(&att.evidence)
                .unwrap_or_else(|| env::panic_str("ERR_BAD_ATTESTATION: evidence is not 0x bytes32")),
            issued_at: att.issued_at,
        }
    }

    fn bounty_ref(&self, id: u32) -> &Bounty {
        self.bounties
            .get(id)
            .unwrap_or_else(|| env::panic_str("ERR_UNKNOWN_BOUNTY"))
    }

    fn bounty_mut(&mut self, id: u32) -> &mut Bounty {
        self.bounties
            .get_mut(id)
            .unwrap_or_else(|| env::panic_str("ERR_UNKNOWN_BOUNTY"))
    }

    fn view(&self, id: u32, b: &Bounty) -> BountyView {
        BountyView {
            id,
            slug: b.slug.clone(),
            project: eip712::to_hex(&b.project),
            builder: eip712::to_hex(&b.builder),
            builder_account: eth_implicit_account(&b.builder),
            creator: b.creator.clone(),
            start: b.start,
            deadline: b.deadline,
            refunds_open_at: b.deadline + CLAIM_GRACE + 1,
            min_ships: b.min_ships,
            payout: b.payout,
            status: b.status,
            total: U128(b.total),
            paid_out: U128(b.paid_out),
            funders: b.funders,
            ships: b.ships,
            evidence: if b.status == Status::Open {
                None
            } else {
                Some(eip712::to_hex(&b.evidence))
            },
            failed_payouts: b.failed_payouts,
        }
    }

    fn internal_fund(&mut self, id: u32, funder: AccountId, amount: u128) {
        require!(!self.retired, "ERR_RETIRED");
        require!(amount > 0, "ERR_NOTHING_RECEIVED");
        let now = now_seconds();
        let b = self.bounty_ref(id);
        require!(b.status == Status::Open, "ERR_NOT_OPEN");
        require!(now < b.deadline, "ERR_FUNDING_CLOSED");
        let key = (id, funder.clone());
        let before = self.contributed.get(&key).copied().unwrap_or(0);
        let after = before
            .checked_add(amount)
            .unwrap_or_else(|| env::panic_str("ERR_OVERFLOW"));
        self.contributed.insert(key, after);
        let b = self.bounty_mut(id);
        b.total = b
            .total
            .checked_add(amount)
            .unwrap_or_else(|| env::panic_str("ERR_OVERFLOW"));
        if before == 0 {
            b.funders += 1;
        }
        let total = b.total;
        emit(
            "funded",
            json!({"id": id, "funder": funder, "amount": U128(amount), "total": U128(total)}),
        );
    }

    /// Start a payout of everything still owed. `claimer` gets back whatever of `attached` is not spent on the
    /// builder's storage registration.
    fn internal_release(&mut self, id: u32, route: Payout, claimer: AccountId, attached: u128) -> Promise {
        let min_gas = if route == Payout::Intents {
            MIN_GAS_INTENTS
        } else {
            MIN_GAS_DIRECT
        };
        require!(
            env::prepaid_gas() >= min_gas,
            format!("ERR_GAS: attach at least {} TGas", min_gas.as_tgas())
        );
        let token = self.token.clone();
        let storage = self.token_storage_deposit;
        let intents = self.intents.clone();
        let b = self.bounty_mut(id);
        require!(b.status == Status::Verified, "ERR_NOT_VERIFIED");
        let amount = b.total - b.paid_out;
        b.status = Status::Paying;
        let builder = eth_implicit_account(&b.builder);
        match route {
            Payout::Near => {
                require!(
                    attached >= storage,
                    format!(
                        "ERR_STORAGE_DEPOSIT: attach at least {} yoctoNEAR for the builder's token storage",
                        storage
                    )
                );
                ext_ft::ext(token)
                    .with_static_gas(GAS_STORAGE_BALANCE_OF)
                    .storage_balance_of(builder)
                    .then(
                        Self::ext(env::current_account_id())
                            .with_static_gas(GAS_ON_STORAGE_CHECKED)
                            .on_storage_checked(id, U128(amount), claimer, U128(attached)),
                    )
            }
            Payout::Intents => {
                let intents = intents.unwrap_or_else(|| env::panic_str("ERR_NO_INTENTS"));
                let transfer = ext_ft::ext(token)
                    .with_attached_deposit(ONE_YOCTO)
                    .with_static_gas(GAS_FT_TRANSFER_CALL)
                    .ft_transfer_call(
                        intents,
                        U128(amount),
                        Some(format!("Proof of Ship bounty {id}")),
                        builder.to_string(),
                    )
                    .then(
                        Self::ext(env::current_account_id())
                            .with_static_gas(GAS_ON_PAYOUT)
                            .on_intents_payout(id, U128(amount)),
                    );
                if attached > 0 {
                    // Nothing to register for an Intents deposit: the claimer's deposit goes straight back.
                    Promise::new(claimer)
                        .transfer(NearToken::from_yoctonear(attached))
                        .detach();
                }
                transfer
            }
        }
    }

    fn settle_payout(&mut self, id: u32, sent: u128, used: u128, route: &str) -> bool {
        let b = self.bounty_mut(id);
        let used = used.min(sent);
        b.paid_out += used;
        let builder = eip712::to_hex(&b.builder);
        if b.paid_out == b.total {
            b.status = Status::Paid;
            let (ships, evidence, paid) = (b.ships, eip712::to_hex(&b.evidence), b.paid_out);
            emit(
                "claimed",
                json!({"id": id, "builder": builder, "route": route, "paid": U128(paid), "ships": ships, "evidence": evidence}),
            );
            true
        } else {
            b.status = Status::Verified;
            if used == 0 {
                b.failed_payouts += 1;
            }
            let (owed, failed) = (b.total - b.paid_out, b.failed_payouts);
            emit(
                "payout_incomplete",
                json!({"id": id, "builder": builder, "route": route, "sent": U128(used), "still_owed": U128(owed), "failed_payouts": failed}),
            );
            false
        }
    }
}

#[near]
impl ShipEscrow {
    /// Deploy and init in one transaction from the escrow account (init is private to it). `attester` is HEY's EVM
    /// signing address, `token` the one NEP-141 token bounties are held in, `chain_id` 397 on mainnet or 398 on
    /// testnet, `guardian` an optional NEAR account that can only retire the escrow, `intents` the NEAR Intents
    /// verifier (`intents.near` on mainnet; none on testnet, where Intents does not exist).
    #[init]
    #[private]
    pub fn new(
        attester: String,
        token: AccountId,
        chain_id: u64,
        guardian: Option<AccountId>,
        intents: Option<AccountId>,
        token_storage_deposit: Option<U128>,
    ) -> Self {
        let attester = eip712::parse_hex::<20>(&attester).unwrap_or_else(|| env::panic_str("ERR_BAD_ATTESTER"));
        require!(attester != [0u8; 20], "ERR_ZERO_ATTESTER");
        require!(
            chain_id == eip712::CHAIN_ID_MAINNET || chain_id == eip712::CHAIN_ID_TESTNET,
            "ERR_CHAIN_ID: 397 (mainnet) or 398 (testnet)"
        );
        let salt = eip712::salt_for(env::current_account_id().as_str());
        Self {
            attester,
            token,
            token_storage_deposit: token_storage_deposit
                .map(|d| d.0)
                .unwrap_or(DEFAULT_TOKEN_STORAGE_DEPOSIT),
            guardian,
            intents,
            chain_id,
            salt,
            domain_separator: eip712::domain_separator(chain_id, &salt),
            retired: false,
            bounties: Vector::new(StorageKey::Bounties),
            contributed: LookupMap::new(StorageKey::Contributed),
        }
    }

    // ---------------------------------------------------------------- bounties

    /// Open a bounty: the builder at EVM address `builder` is paid once HEY counts `min_ships` ships for `slug` from
    /// now until `deadline` (unix seconds). Attach NEAR for the bounty's storage (0.01 NEAR is plenty; the rest comes
    /// back). Fund it afterwards with `ft_transfer_call` and `{"action":"fund","id":<id>}`.
    #[payable]
    pub fn create(
        &mut self,
        builder: String,
        slug: String,
        min_ships: u32,
        deadline: u64,
        payout: Option<Payout>,
    ) -> u32 {
        require!(!self.retired, "ERR_RETIRED");
        let builder = eip712::parse_hex::<20>(&builder).unwrap_or_else(|| env::panic_str("ERR_BAD_BUILDER"));
        require!(builder != [0u8; 20] && builder != DEAD_ADDRESS, "ERR_BAD_BUILDER");
        require!(
            is_hey_slug(&slug),
            "ERR_BAD_SLUG: a HEY slug, ^[a-z0-9][a-z0-9-]{0,127}$"
        );
        require!(min_ships > 0, "ERR_BAD_MIN_SHIPS");
        let now = now_seconds();
        require!(
            deadline >= now + MIN_DURATION && deadline <= now + MAX_DURATION,
            "ERR_BAD_DEADLINE"
        );
        let payout = payout.unwrap_or(Payout::Near);
        require!(payout == Payout::Near || self.intents.is_some(), "ERR_NO_INTENTS");

        let before = env::storage_usage();
        let id = self.bounties.len();
        let project = eip712::keccak(slug.as_bytes());
        let creator = env::predecessor_account_id();
        self.bounties.push(Bounty {
            builder,
            project,
            slug: slug.clone(),
            creator: creator.clone(),
            start: now,
            deadline,
            min_ships,
            payout,
            status: Status::Open,
            total: 0,
            paid_out: 0,
            funders: 0,
            ships: 0,
            evidence: [0u8; 32],
            failed_payouts: 0,
        });
        self.bounties.flush();
        let cost = env::storage_byte_cost().saturating_mul(u128::from(env::storage_usage() - before));
        let attached = env::attached_deposit();
        require!(
            attached >= cost,
            format!("ERR_STORAGE: attach at least {} yoctoNEAR", cost.as_yoctonear())
        );
        let excess = attached.saturating_sub(cost);
        if !excess.is_zero() {
            Promise::new(creator.clone()).transfer(excess).detach();
        }
        emit(
            "bounty_created",
            json!({
                "id": id, "slug": slug, "project": eip712::to_hex(&project), "builder": eip712::to_hex(&builder),
                "creator": creator, "min_ships": min_ships, "start": now, "deadline": deadline, "payout": payout,
            }),
        );
        id
    }

    /// Release a bounty with HEY's attestation. Anyone may submit it: the tokens always go to the builder the
    /// signature names. For a `near` payout attach the token's storage deposit (0.00125 NEAR for USDC), which comes
    /// back if the builder is already registered; attach at least 100 TGas (130 for an `intents` payout).
    #[payable]
    pub fn claim(&mut self, id: u32, attestation: Attestation, signature: String) -> Promise {
        require!(!self.retired, "ERR_RETIRED");
        let now = now_seconds();
        let att = Self::parse_attestation(&attestation);
        let sig =
            eip712::parse_hex_vec(&signature).unwrap_or_else(|| env::panic_str(sig_error_message(SigError::Malformed)));
        let digest = eip712::digest(&self.domain_separator, &att);
        let attester = self.attester;
        let b = self.bounty_ref(id);
        require!(b.status == Status::Open, "ERR_ALREADY_CLAIMED");
        require!(now <= b.deadline + CLAIM_GRACE, "ERR_CLAIM_CLOSED");
        require!(b.total > 0, "ERR_NOTHING_FUNDED");
        require!(att.project == b.project, "ERR_WRONG_PROJECT");
        require!(att.builder == b.builder, "ERR_WRONG_BUILDER");
        require!(
            att.since == b.start && att.until >= att.since && att.until <= b.deadline && att.until <= att.issued_at,
            "ERR_WRONG_WINDOW"
        );
        require!(
            att.issued_at <= now && now - att.issued_at <= MAX_ATTESTATION_AGE,
            "ERR_STALE_ATTESTATION"
        );
        require!(att.ships >= b.min_ships, "ERR_NOT_ENOUGH_SHIPS");
        let signer = eip712::recover_address(&digest, &sig).unwrap_or_else(|e| env::panic_str(sig_error_message(e)));
        require!(
            signer == attester,
            "ERR_WRONG_SIGNER: not the attester's signature over this domain and message"
        );

        let route = b.payout;
        let b = self.bounty_mut(id);
        b.status = Status::Verified;
        b.ships = att.ships;
        b.evidence = att.evidence;
        emit(
            "claim_verified",
            json!({"id": id, "builder": eip712::to_hex(&att.builder), "ships": att.ships, "evidence": eip712::to_hex(&att.evidence), "until": att.until, "issued_at": att.issued_at}),
        );
        self.internal_release(
            id,
            route,
            env::predecessor_account_id(),
            env::attached_deposit().as_yoctonear(),
        )
    }

    /// Retry the payout of a verified bounty whose payout did not go through. Anyone may call it. `direct: true`
    /// pays an `intents` bounty to the builder's eth-implicit account instead, allowed only after an Intents
    /// deposit failed; the recipient key is the same either way.
    #[payable]
    pub fn release(&mut self, id: u32, direct: Option<bool>) -> Promise {
        let b = self.bounty_ref(id);
        require!(b.status == Status::Verified, "ERR_NOT_VERIFIED");
        let route = if direct == Some(true) {
            require!(
                b.payout == Payout::Intents && b.failed_payouts > 0,
                "ERR_DIRECT_NOT_ALLOWED"
            );
            Payout::Near
        } else {
            b.payout
        };
        self.internal_release(
            id,
            route,
            env::predecessor_account_id(),
            env::attached_deposit().as_yoctonear(),
        )
    }

    /// Take back your contribution once the claim window has closed without a claim (or at once after retirement).
    pub fn refund(&mut self, id: u32) -> Promise {
        let funder = env::predecessor_account_id();
        let now = now_seconds();
        let retired = self.retired;
        let token = self.token.clone();
        let b = self.bounty_ref(id);
        require!(b.status == Status::Open, "ERR_ALREADY_CLAIMED");
        require!(retired || now > b.deadline + CLAIM_GRACE, "ERR_REFUND_NOT_OPEN");
        let amount = self.contributed.remove(&(id, funder.clone())).unwrap_or(0);
        require!(amount > 0, "ERR_NOTHING_TO_REFUND");
        let b = self.bounty_mut(id);
        b.total -= amount;
        b.funders -= 1;
        ext_ft::ext(token)
            .with_attached_deposit(ONE_YOCTO)
            .with_static_gas(GAS_FT_TRANSFER)
            .ft_transfer(
                funder.clone(),
                U128(amount),
                Some(format!("Proof of Ship refund, bounty {id}")),
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_ON_REFUND)
                    .on_refund(id, funder, U128(amount)),
            )
    }

    /// Stop all claims for good and open refunds now. Guardian only, with one yoctoNEAR (a full-access key); once.
    #[payable]
    pub fn retire(&mut self) {
        near_sdk::assert_one_yocto();
        require!(
            self.guardian.as_ref() == Some(&env::predecessor_account_id()),
            "ERR_NOT_GUARDIAN"
        );
        require!(!self.retired, "ERR_RETIRED");
        self.retired = true;
        emit("escrow_retired", json!({}));
    }

    // ---------------------------------------------------------------- callbacks

    #[private]
    pub fn on_storage_checked(
        &mut self,
        #[callback_result] registered: Result<Option<Value>, PromiseError>,
        id: u32,
        amount: U128,
        claimer: AccountId,
        attached: U128,
    ) -> PromiseOrValue<bool> {
        let builder = eth_implicit_account(&self.bounty_ref(id).builder);
        let token = self.token.clone();
        // Note: `a.then(b)` returns `b`, so a chain is built left to right: a.then(b).then(c).
        let ft_transfer = || {
            ext_ft::ext(token.clone())
                .with_attached_deposit(ONE_YOCTO)
                .with_static_gas(GAS_FT_TRANSFER)
                .ft_transfer(builder.clone(), amount, Some(format!("Proof of Ship bounty {id}")))
        };
        let settle = || {
            Self::ext(env::current_account_id())
                .with_static_gas(GAS_ON_PAYOUT)
                .on_direct_payout(id, amount)
        };
        let refund_claimer = |yocto: u128| {
            if yocto > 0 {
                Promise::new(claimer.clone())
                    .transfer(NearToken::from_yoctonear(yocto))
                    .detach();
            }
        };
        match registered {
            Err(_) => {
                // The token did not answer: nothing moved. Back to Verified, deposit back to the claimer.
                self.settle_payout(id, amount.0, 0, "near");
                refund_claimer(attached.0);
                PromiseOrValue::Value(false)
            }
            Ok(Some(_)) => {
                refund_claimer(attached.0);
                emit(
                    "payout_sent",
                    json!({"id": id, "route": "near", "builder_registered": true, "deposit_returned": attached}),
                );
                PromiseOrValue::Promise(ft_transfer().then(settle()))
            }
            Ok(None) => {
                let storage = self.token_storage_deposit;
                refund_claimer(attached.0.saturating_sub(storage));
                emit(
                    "payout_sent",
                    json!({"id": id, "route": "near", "builder_registered": false, "deposit_returned": U128(attached.0.saturating_sub(storage))}),
                );
                let register = ext_ft::ext(token.clone())
                    .with_attached_deposit(NearToken::from_yoctonear(storage))
                    .with_static_gas(GAS_STORAGE_DEPOSIT)
                    .storage_deposit(Some(builder.clone()), Some(true));
                PromiseOrValue::Promise(register.then(ft_transfer()).then(settle()))
            }
        }
    }

    #[private]
    pub fn on_direct_payout(&mut self, id: u32, amount: U128) -> bool {
        // ft_transfer answers nothing; anything but a failure means the tokens moved.
        let used = match env::promise_result_checked(0, 64) {
            Err(PromiseError::Failed) => 0,
            _ => amount.0,
        };
        self.settle_payout(id, amount.0, used, "near")
    }

    #[private]
    pub fn on_intents_payout(&mut self, id: u32, amount: U128) -> bool {
        // ft_transfer_call answers how much the receiver kept; the rest was refunded to the escrow.
        let used = match env::promise_result_checked(0, 128) {
            Ok(bytes) => serde_json::from_slice::<U128>(&bytes).map(|u| u.0).unwrap_or(0),
            Err(_) => 0,
        };
        self.settle_payout(id, amount.0, used, "intents")
    }

    #[private]
    pub fn on_refund(&mut self, id: u32, funder: AccountId, amount: U128) -> bool {
        if !matches!(env::promise_result_checked(0, 64), Err(PromiseError::Failed)) {
            emit("refunded", json!({"id": id, "funder": funder, "amount": amount}));
            return true;
        }
        // The transfer failed (the funder unregistered from the token?): their contribution stands.
        let key = (id, funder.clone());
        let prev = self.contributed.get(&key).copied().unwrap_or(0);
        self.contributed.insert(key, prev + amount.0);
        let b = self.bounty_mut(id);
        b.total += amount.0;
        if prev == 0 {
            b.funders += 1;
        }
        emit("refund_failed", json!({"id": id, "funder": funder, "amount": amount}));
        false
    }

    // ---------------------------------------------------------------- views

    pub fn get_config(&self) -> ConfigView {
        ConfigView {
            attester: eip712::to_hex(&self.attester),
            token: self.token.clone(),
            chain_id: self.chain_id,
            salt: eip712::to_hex(&self.salt),
            domain_separator: eip712::to_hex(&self.domain_separator),
            guardian: self.guardian.clone(),
            intents: self.intents.clone(),
            token_storage_deposit: U128(self.token_storage_deposit),
            retired: self.retired,
            max_attestation_age: MAX_ATTESTATION_AGE,
            claim_grace: CLAIM_GRACE,
            min_duration: MIN_DURATION,
            max_duration: MAX_DURATION,
        }
    }

    pub fn bounty_count(&self) -> u32 {
        self.bounties.len()
    }

    pub fn get_bounty(&self, id: u32) -> Option<BountyView> {
        self.bounties.get(id).map(|b| self.view(id, b))
    }

    /// Bounties from `from_index`, at most `limit` (default and cap 50).
    pub fn get_bounties(&self, from_index: Option<u32>, limit: Option<u32>) -> Vec<BountyView> {
        let from = from_index.unwrap_or(0);
        let limit = limit.unwrap_or(50).min(50);
        (from..self.bounties.len().min(from.saturating_add(limit)))
            .filter_map(|id| self.bounties.get(id).map(|b| self.view(id, b)))
            .collect()
    }

    pub fn get_contribution(&self, id: u32, account_id: AccountId) -> U128 {
        U128(self.contributed.get(&(id, account_id)).copied().unwrap_or(0))
    }

    /// The EIP-712 digest HEY signs for `attestation` on this escrow.
    pub fn digest(&self, attestation: Attestation) -> String {
        eip712::to_hex(&eip712::digest(
            &self.domain_separator,
            &Self::parse_attestation(&attestation),
        ))
    }

    /// Who signed `attestation` for this escrow, or why no one did: a dry run of the signature half of `claim`.
    pub fn recover_signer(&self, attestation: Attestation, signature: String) -> SignerView {
        let digest = eip712::digest(&self.domain_separator, &Self::parse_attestation(&attestation));
        let recovered = match eip712::parse_hex_vec(&signature) {
            None => Err(SigError::Malformed),
            Some(sig) => eip712::recover_address(&digest, &sig),
        };
        match recovered {
            Ok(a) => SignerView {
                signer: Some(eip712::to_hex(&a)),
                is_attester: a == self.attester,
                error: None,
                digest: eip712::to_hex(&digest),
            },
            Err(e) => SignerView {
                signer: None,
                is_attester: false,
                error: Some(sig_error_message(e).to_string()),
                digest: eip712::to_hex(&digest),
            },
        }
    }
}

#[near]
impl FungibleTokenReceiver for ShipEscrow {
    /// Fund a bounty: `ft_transfer_call` on the escrow's token with `msg` `{"action":"fund","id":<id>}`. Anything
    /// else, or a bounty that cannot take funds, panics and the token refunds the sender in full.
    fn ft_on_transfer(&mut self, sender_id: AccountId, amount: U128, msg: String) -> PromiseOrValue<U128> {
        require!(env::predecessor_account_id() == self.token, "ERR_WRONG_TOKEN");
        let action: FtAction = serde_json::from_str(&msg)
            .unwrap_or_else(|_| env::panic_str(r#"ERR_BAD_MSG: {"action":"fund","id":<id>}"#));
        match action {
            FtAction::Fund { id } => self.internal_fund(id, sender_id, amount.0),
        }
        PromiseOrValue::Value(U128(0))
    }
}

#[cfg(test)]
mod tests;
