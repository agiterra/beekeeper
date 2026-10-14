// What does this relay speak? Unauthenticated NIP-11, then one unauthenticated
// /query to show the bridge refuses it. No key is needed.
//
//   node foreign-info.mjs [kind ...]        assert each kind is in supported_kinds
import { relayHttpUrl, writeJson } from './_common.mjs';

const origin = relayHttpUrl();
const info = await (await fetch(origin + '/', { headers: { Accept: 'application/nostr+json' } })).json();
const kinds = Array.isArray(info.supported_kinds) ? info.supported_kinds : null;
if (!kinds) throw new Error('NIP-11 has no supported_kinds: this relay predates the field');
const byKind = new Map(kinds.map((entry) => [entry.kind, entry]));
const wanted = process.argv.slice(2).map(Number);
const missing = wanted.filter((kind) => !byKind.has(kind));
if (missing.length) throw new Error('supported_kinds lacks ' + missing.join(','));

// The bridge is NIP-98 authenticated: an unsigned read is refused, not answered.
const unauth = await fetch(origin + '/query', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '[{"kinds":[44224],"limit":1}]' });

const summary = {
  relay: origin,
  software_commit: info.software_commit,
  self: info.self ?? null,
  supported_nips: info.supported_nips,
  supported_kinds_count: kinds.length,
  requested: wanted.map((kind) => byKind.get(kind)),
  unauthenticated_query_status: unauth.status,
};
writeJson('foreign-info.json', { nip11: info, summary });
console.log(JSON.stringify(summary));
