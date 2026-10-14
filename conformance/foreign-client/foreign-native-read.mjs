// Relay-only authenticated read of any filter, with every signature verified.
//
//   node foreign-native-read.mjs '<filter json>' <output file name>
//
// A filter must name `kinds`: the relay refuses one that does not. Prints the
// matched events as `{id, kind, content}` and writes the full signed answer.
import { loadKey, nip98Client, relayHttpUrl, verifyAll, writeJson } from './_common.mjs';

const key = loadKey();
const http = nip98Client(key, relayHttpUrl());
const filter = JSON.parse(process.argv[2] || 'null');
const out = process.argv[3];
if (!filter || !out) throw new Error('usage: foreign-native-read.mjs <filter json> <output name>');
const result = await http('/query', Array.isArray(filter) ? filter : [filter]);
if (result.status !== 200 || !Array.isArray(result.body)) throw new Error(JSON.stringify(result));
verifyAll(result.body);
writeJson(out, result);
console.log(JSON.stringify(result.body.map((e) => ({ id: e.id, kind: e.kind, content: e.content }))));
