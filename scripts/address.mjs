#!/usr/bin/env node
// Print a fresh random EVM address (its key is thrown away): a builder for a demo bounty.
import { generatePrivateKey, privateKeyToAccount } from 'viem/accounts';
console.log(privateKeyToAccount(generatePrivateKey()).address);
