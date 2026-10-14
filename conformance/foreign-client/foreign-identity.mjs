// Print the public key of the configured client key, and nothing else. Used
// by scripts that need to name a key without ever printing the secret.
import { getPublicKey, loadKey } from './_common.mjs';

console.log(JSON.stringify({ pubkey: getPublicKey(loadKey()) }));
