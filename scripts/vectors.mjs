#!/usr/bin/env node
// Regenerate contract/src/vectors.json: EIP-712 digests and signatures made by viem, an implementation independent of
// the contract's own, so the Rust unit tests prove the escrow hashes exactly what HEY signs.
import { writeFileSync } from 'node:fs';
import { hashTypedData, hashDomain, keccak256, stringToBytes } from 'viem';
import { privateKeyToAccount } from 'viem/accounts';
import { SHIPS_TYPES, TEST_KEY, buildAttestation, nearDomain, signAttestation } from './sign.mjs';

const escrow = 'ship-escrow.hey.near';
const attestation = buildAttestation({
  slug: 'predict-fwa',
  builder: '0x1111111111111111111111111111111111111111',
  since: 1762300000,
  until: 1762900000,
  ships: 3,
  evidenceIds: ['ship:7d7b0a52-1c55-4f3e-9d1e-5a0b8f3c2e11', 'ship:0b9e7c43-2d1a-4b6f-8e3d-1f2a3b4c5d6e'],
  issuedAt: 1762900060,
});
const mainnet = await signAttestation(attestation, { escrow, chainId: 397 });
const testnet = await signAttestation(attestation, { escrow, chainId: 398 });
const otherEscrow = await signAttestation(attestation, { escrow: 'other-escrow.near', chainId: 397 });
// The Robinhood Chain ShipEscrow's domain: chain 4663 and its contract as verifyingContract.
const rhDomain = { name: 'HEY Proof of Ship', version: '1', chainId: 4663, verifyingContract: '0x3Da17a89F1d29aB96419269514C798a8dd6125f2' };
const msg = { ...attestation, since: BigInt(attestation.since), until: BigInt(attestation.until), issuedAt: BigInt(attestation.issuedAt) };
const robinhood = {
  signature: await privateKeyToAccount(TEST_KEY).signTypedData({ domain: rhDomain, types: SHIPS_TYPES, primaryType: 'Ships', message: msg }),
  digest: hashTypedData({ domain: rhDomain, types: SHIPS_TYPES, primaryType: 'Ships', message: msg }),
};
const out = {
  escrow,
  attester: mainnet.signer,
  salt: keccak256(stringToBytes(escrow)),
  domainSeparator: {
    397: hashDomain({ domain: nearDomain(escrow, 397), types: { EIP712Domain: [{ name: 'name', type: 'string' }, { name: 'version', type: 'string' }, { name: 'chainId', type: 'uint256' }, { name: 'salt', type: 'bytes32' }] } }),
  },
  attestation,
  mainnet: { digest: mainnet.digest, signature: mainnet.signature },
  testnet: { digest: testnet.digest, signature: testnet.signature },
  otherEscrow: { digest: otherEscrow.digest, signature: otherEscrow.signature },
  robinhood,
};
writeFileSync(new URL('../contract/src/vectors.json', import.meta.url), JSON.stringify(out, null, 2) + '\n');
console.log(JSON.stringify(out, null, 2));
