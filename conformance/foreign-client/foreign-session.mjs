// Relay-only session client: no host socket, service files, or Beekeeper SDK.
//
//   node foreign-session.mjs read
//       authenticated read of 44222/44223/44224/44225 in FOREIGN_CLIENT_CHANNEL
//   node foreign-session.mjs <stop|resume|restart> <operation> '<target json>' <authorityPubkey>
//       publish one kind:44221 lifecycle command for an exact public target
//
// Env: RELAY_URL, FOREIGN_CLIENT_CHANNEL, a key (see _common.mjs). The
// relay's answer — accepted or refused — is printed and written; this script
// asserts only on signatures and shape, never on the outcome.
import { finalizeEvent, loadKey, nip98Client, now, relayHttpUrl, requireEnv, requireUuid, verifyAll, writeJson } from './_common.mjs';

const key = loadKey();
const http = nip98Client(key, relayHttpUrl());
const channel = requireUuid('FOREIGN_CLIENT_CHANNEL', requireEnv('FOREIGN_CLIENT_CHANNEL'));
const action = process.argv[2] || 'read';
const label = process.env.FOREIGN_CLIENT_LABEL || 'client';

let result;
if (action === 'read') {
  result = await http('/query', [{ kinds: [44222, 44223, 44224, 44225], '#h': [channel], limit: 100 }]);
  if (Array.isArray(result.body)) verifyAll(result.body);
} else {
  if (!['stop', 'resume', 'restart'].includes(action)) throw new Error('action must be read, stop, resume or restart');
  const operation = process.argv[3];
  if (!operation) throw new Error('stable operation id required');
  const target = JSON.parse(process.argv[4] || 'null');
  const authority = process.argv[5];
  if (!target || !authority) throw new Error('target json and provider authority pubkey required');
  const payload = { schema: 'buzz-coding-session-lifecycle-command/v1', commandId: operation, action: { type: 'session.' + action, session: target, providerAuthorityPubkey: authority } };
  const event = finalizeEvent({ kind: 44221, created_at: now(), tags: [['h', channel], ['csl-v', 'csl1-1'], ['csl-command', operation]], content: JSON.stringify(payload) }, key);
  result = { event, response: await http('/events', event) };
}
writeJson(`foreign-session-${label}-${action}.json`, result);
console.log(JSON.stringify({ identity: label, action, status: result.status ?? result.response.status, count: Array.isArray(result.body) ? result.body.length : undefined, response: result.response?.body ?? (Array.isArray(result.body) ? undefined : result.body) }));
