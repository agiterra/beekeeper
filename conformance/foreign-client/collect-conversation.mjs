// Collect and verify every conversation and coding-session record in one
// channel: kinds 9 and 44220/44221/44223/44224/44225/44234. Relay-only.
//
//   node collect-conversation.mjs [label]
import { loadKey, nip98Client, relayHttpUrl, requireEnv, requireUuid, verifyAll, writeJson } from './_common.mjs';

const key = loadKey();
const http = nip98Client(key, relayHttpUrl());
const channel = requireUuid('FOREIGN_CLIENT_CHANNEL', requireEnv('FOREIGN_CLIENT_CHANNEL'));
const label = process.argv[2] || 'current';
if (!/^[a-z0-9-]+$/.test(label)) throw new Error('invalid label');
const answer = await http('/query', [{ kinds: [9, 44220, 44221, 44223, 44224, 44225, 44234], '#h': [channel], limit: 5000 }]);
if (answer.status !== 200) throw new Error('query refused: ' + JSON.stringify(answer));
const rows = verifyAll(answer.body);
writeJson('conversation-signed-' + label + '.json', rows);
console.log(JSON.stringify({ label, verifiedEventCount: rows.length, kinds: [...new Set(rows.map((x) => x.kind))].sort((a, b) => a - b) }));
