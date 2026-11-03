//! The EIP-712 encoding HEY Research Lab signs, and the secp256k1 checks that make a signature HEY's.
//!
//! Domain: `EIP712Domain(string name,string version,uint256 chainId,bytes32 salt)` with name "HEY Proof of Ship",
//! version "1", chainId 397 (NEAR mainnet) or 398 (NEAR testnet), and salt = keccak256 of the escrow's NEAR account id
//! (its UTF-8 bytes). NEAR has no EVM contract address to put in `verifyingContract`, so the salt binds a signature to
//! one escrow account instead.
//!
//! Message: `Ships(bytes32 project,address builder,uint64 since,uint64 until,uint32 ships,bytes32 evidence,uint64 issuedAt)`,
//! the same struct the Robinhood Chain ShipEscrow verifies.

use near_sdk::env;

pub const DOMAIN_NAME: &str = "HEY Proof of Ship";
pub const DOMAIN_VERSION: &str = "1";
pub const DOMAIN_TYPE: &str = "EIP712Domain(string name,string version,uint256 chainId,bytes32 salt)";
pub const SHIPS_TYPE: &str =
    "Ships(bytes32 project,address builder,uint64 since,uint64 until,uint32 ships,bytes32 evidence,uint64 issuedAt)";

/// NEAR mainnet and testnet, as EIP-712 chain ids.
pub const CHAIN_ID_MAINNET: u64 = 397;
pub const CHAIN_ID_TESTNET: u64 = 398;

/// secp256k1 group order n, divided by two (big-endian). A signature whose `s` is above it is the malleable twin
/// of a valid one and is refused (EIP-2).
const SECP256K1_HALF_N: [u8; 32] = [
    0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x5d, 0x57, 0x6e,
    0x73, 0x57, 0xa4, 0x50, 0x1d, 0xdf, 0xe9, 0x2f, 0x46, 0x68, 0x1b, 0x20, 0xa0,
];

/// The signed message, parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ships {
    pub project: [u8; 32],
    pub builder: [u8; 20],
    pub since: u64,
    pub until: u64,
    pub ships: u32,
    pub evidence: [u8; 32],
    pub issued_at: u64,
}

pub fn keccak(bytes: &[u8]) -> [u8; 32] {
    env::keccak256_array(bytes)
}

fn word_u64(v: u64) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[24..].copy_from_slice(&v.to_be_bytes());
    w
}

fn word_address(a: &[u8; 20]) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[12..].copy_from_slice(a);
    w
}

/// `keccak256(utf8(account_id))`: the domain's salt for one escrow account.
pub fn salt_for(account_id: &str) -> [u8; 32] {
    keccak(account_id.as_bytes())
}

pub fn domain_separator(chain_id: u64, salt: &[u8; 32]) -> [u8; 32] {
    let mut buf = Vec::with_capacity(32 * 5);
    buf.extend_from_slice(&keccak(DOMAIN_TYPE.as_bytes()));
    buf.extend_from_slice(&keccak(DOMAIN_NAME.as_bytes()));
    buf.extend_from_slice(&keccak(DOMAIN_VERSION.as_bytes()));
    buf.extend_from_slice(&word_u64(chain_id));
    buf.extend_from_slice(salt);
    keccak(&buf)
}

pub fn struct_hash(m: &Ships) -> [u8; 32] {
    let mut buf = Vec::with_capacity(32 * 8);
    buf.extend_from_slice(&keccak(SHIPS_TYPE.as_bytes()));
    buf.extend_from_slice(&m.project);
    buf.extend_from_slice(&word_address(&m.builder));
    buf.extend_from_slice(&word_u64(m.since));
    buf.extend_from_slice(&word_u64(m.until));
    buf.extend_from_slice(&word_u64(u64::from(m.ships)));
    buf.extend_from_slice(&m.evidence);
    buf.extend_from_slice(&word_u64(m.issued_at));
    keccak(&buf)
}

/// The EIP-712 digest: `keccak256(0x19 0x01 || domainSeparator || structHash)`.
pub fn digest(domain_separator: &[u8; 32], m: &Ships) -> [u8; 32] {
    let mut buf = Vec::with_capacity(66);
    buf.extend_from_slice(&[0x19, 0x01]);
    buf.extend_from_slice(domain_separator);
    buf.extend_from_slice(&struct_hash(m));
    keccak(&buf)
}

/// Why a signature is not accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SigError {
    /// Not 65 bytes of hex.
    Malformed,
    /// `v` is not 27, 28, 0 or 1.
    BadV,
    /// `r` or `s` is zero.
    ZeroComponent,
    /// `s` is in the upper half of the curve order (the malleable twin).
    HighS,
    /// The host could not recover a key.
    Unrecoverable,
}

/// Recover the EVM address that signed `digest` with a 65-byte `r || s || v` signature. High-s signatures are
/// refused before the host is asked, and the host is asked with its own malleability check on as well.
pub fn recover_address(digest: &[u8; 32], sig: &[u8]) -> Result<[u8; 20], SigError> {
    if sig.len() != 65 {
        return Err(SigError::Malformed);
    }
    let v = match sig[64] {
        27 | 28 => sig[64] - 27,
        0 | 1 => sig[64],
        _ => return Err(SigError::BadV),
    };
    let r = &sig[0..32];
    let s = &sig[32..64];
    if r.iter().all(|b| *b == 0) || s.iter().all(|b| *b == 0) {
        return Err(SigError::ZeroComponent);
    }
    if s > &SECP256K1_HALF_N[..] {
        return Err(SigError::HighS);
    }
    let pk = env::ecrecover(digest, &sig[..64], v, true).ok_or(SigError::Unrecoverable)?;
    let h = keccak(&pk);
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&h[12..]);
    Ok(addr)
}

/// Parse `0x` + exactly `2 * N` hex characters (either case).
pub fn parse_hex<const N: usize>(s: &str) -> Option<[u8; N]> {
    let body = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))?;
    if body.len() != 2 * N {
        return None;
    }
    let mut out = [0u8; N];
    hex::decode_to_slice(body, &mut out).ok()?;
    Some(out)
}

pub fn parse_hex_vec(s: &str) -> Option<Vec<u8>> {
    let body = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))?;
    hex::decode(body).ok()
}

pub fn to_hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}
