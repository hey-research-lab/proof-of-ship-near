#!/usr/bin/env node
// Sign a Proof of Ship attestation for the NEAR escrow exactly as HEY Research Lab signs it: EIP-712 typed data,
// domain { name: "HEY Proof of Ship", version: "1", chainId: 397 | 398, salt: keccak256(utf8(escrowAccountId)) }.
//
//   node scripts/sign.mjs --escrow ship-escrow.testnet --chain 398 --slug predict-fwa \
//     --builder 0x... --since 1762300000 --until 1762390000 --ships 3
//
// Prints { attestation, signature, digest, signer } (or, with --print view-args | claim-args, the JSON arguments for the
// contract's recover_signer view or claim call) with the attestation in the shape HEY's
// GET /api/v1/proof-of-ship/attestation returns it. The key comes from ATTESTER_KEY; without it the PUBLIC test key
// keccak256("hey-proof-of-ship-near test attester") is used, which only a test or testnet escrow trusts.

import { hashTypedData, keccak256, stringToBytes, getAddress } from 'viem';
import { privateKeyToAccount } from 'viem/accounts';

export const TEST_KEY = keccak256(stringToBytes('hey-proof-of-ship-near test attester'));

export const SHIPS_TYPES = {
  Ships: [
    { name: 'project', type: 'bytes32' },
    { name: 'builder', type: 'address' },
    { name: 'since', type: 'uint64' },
    { name: 'until', type: 'uint64' },
    { name: 'ships', type: 'uint32' },
    { name: 'evidence', type: 'bytes32' },
    { name: 'issuedAt', type: 'uint64' },
  ],
};

export const nearDomain = (escrow, chainId) => ({
  name: 'HEY Proof of Ship',
  version: '1',
  chainId,
  salt: keccak256(stringToBytes(escrow)),
});

export function buildAttestation({ slug, builder, since, until, ships, evidenceIds = [], evidence, issuedAt }) {
  return {
    project: keccak256(stringToBytes(slug)),
    builder: getAddress(builder),
    since: Number(since),
    until: Number(until),
    ships: Number(ships),
    // HEY hashes the sorted evidence ids joined by commas.
    evidence: evidence ?? keccak256(stringToBytes([...evidenceIds].sort().join(','))),
    issuedAt: Number(issuedAt),
  };
}

const message = (a) => ({ ...a, since: BigInt(a.since), until: BigInt(a.until), issuedAt: BigInt(a.issuedAt) });

export async function signAttestation(attestation, { escrow, chainId, key = TEST_KEY }) {
  const account = privateKeyToAccount(key);
  const domain = nearDomain(escrow, chainId);
  const typed = { domain, types: SHIPS_TYPES, primaryType: 'Ships', message: message(attestation) };
  return {
    attestation,
    signature: await account.signTypedData(typed),
    digest: hashTypedData(typed),
    signer: account.address,
  };
}

function args(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 2) {
    if (!argv[i].startsWith('--')) throw new Error(`unexpected argument ${argv[i]}`);
    out[argv[i].slice(2)] = argv[i + 1];
  }
  return out;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const a = args(process.argv.slice(2));
  for (const k of ['escrow', 'slug', 'builder', 'since', 'until', 'ships']) {
    if (a[k] === undefined) {
      console.error(`missing --${k}`);
      process.exit(2);
    }
  }
  const key = process.env.ATTESTER_KEY ?? TEST_KEY;
  if (!process.env.ATTESTER_KEY) console.error('note: signing with the public test key (ATTESTER_KEY not set)');
  const now = Math.floor(Date.now() / 1000);
  const attestation = buildAttestation({
    slug: a.slug,
    builder: a.builder,
    since: a.since,
    until: a.until,
    ships: a.ships,
    evidenceIds: a['evidence-ids'] ? a['evidence-ids'].split(',') : ['ship:demo'],
    issuedAt: a['issued-at'] ?? now - 60,
  });
  const out = await signAttestation(attestation, { escrow: a.escrow, chainId: Number(a.chain ?? 398), key });
  // --print view-args: {attestation, signature} for the recover_signer and digest views;
  // --print claim-args --id N: {id, attestation, signature} for claim.
  if (a.print === 'view-args') console.log(JSON.stringify({ attestation: out.attestation, signature: out.signature }));
  else if (a.print === 'claim-args') console.log(JSON.stringify({ id: Number(a.id ?? 0), attestation: out.attestation, signature: out.signature }));
  else console.log(JSON.stringify(out, null, 2));
}
