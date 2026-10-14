// Independent host-step participant (NIP-HS). No Beekeeper SDK/host/layout.
//
//   node foreign-hook.mjs correlate <runId>
//       read 46013/46014/46022/46023 in FOREIGN_CLIENT_CHANNEL, verify every
//       signature, and prove request -> accepted claim -> result -> relay
//       echo correlate for <runId>. Exit 3 when the exchange is incomplete.
//   node foreign-hook.mjs claim <runId> <stepId> <requestedEventId>
//       claim that request and, if the claim is admitted, answer it with a
//       truthful `refused` result (this client does not execute commands).
//       Prints the relay's answers; a refused claim is a result, not an error.
//
// The witnessed relay key for the echo check is FOREIGN_CLIENT_RELAY_PUBKEY.
// Without it the key is taken from this relay's own NIP-11 `self`, which is
// disclosed in the output as unwitnessed.
import { finalizeEvent, loadKey, nip98Client, now, relayHttpUrl, requireEnv, requireUuid, verifyAll, writeJson } from './_common.mjs';

const key = loadKey();
const origin = relayHttpUrl();
const http = nip98Client(key, origin);
const channel = requireUuid('FOREIGN_CLIENT_CHANNEL', requireEnv('FOREIGN_CLIENT_CHANNEL'));
const mode = process.argv[2];
const SCHEMA = 'buzz-host-step/v1';

let relayPubkey = process.env.FOREIGN_CLIENT_RELAY_PUBKEY;
let witnessed = true;
if (!relayPubkey) {
  const info = await (await fetch(origin + '/', { headers: { Accept: 'application/nostr+json' } })).json();
  relayPubkey = info.self;
  witnessed = false;
  if (!relayPubkey) throw new Error('relay advertises no NIP-11 self and FOREIGN_CLIENT_RELAY_PUBKEY is unset');
}

if (mode === 'claim') {
  const [runId, stepId, requestedEventId] = process.argv.slice(3);
  if (!runId || !stepId || !/^[0-9a-f]{64}$/.test(requestedEventId || '')) throw new Error('usage: claim <runId> <stepId> <requestedEventId hex>');
  requireUuid('runId', runId);
  const tags = [['d', runId + ':' + stepId], ['e', requestedEventId], ['h', channel], ['schema', SCHEMA]];
  const common = { schema: SCHEMA, runId, stepId, requestedEventId, channelId: channel };
  const claim = finalizeEvent({ kind: 46022, created_at: now(), tags, content: JSON.stringify({ ...common, host: { name: 'Independent protocol conformance client' } }) }, key);
  const claimAnswer = await http('/events', claim);
  const admitted = claimAnswer.status === 200 && claimAnswer.body?.accepted === true;
  writeJson('hook-foreign-claim.json', { event: claim, response: claimAnswer });
  let resultAnswer = null;
  let result = null;
  if (admitted) {
    result = finalizeEvent({ kind: 46023, created_at: now(), tags, content: JSON.stringify({ ...common, claimEventId: claim.id, disposition: 'refused', refusal: { code: 'FOREIGN_EXECUTION_UNSUPPORTED', message: 'This protocol client verifies and exchanges the contract; it does not execute commands.' }, timedOut: false }) }, key);
    resultAnswer = await http('/events', result);
    writeJson('hook-foreign-result.json', { event: result, response: resultAnswer });
  }
  console.log(JSON.stringify({ mode, runId, stepId, claimEventId: claim.id, claimAdmitted: admitted, claimResponse: claimAnswer.body, resultEventId: result?.id ?? null, resultResponse: resultAnswer?.body ?? null, execution: 'unsupported' }));
} else if (mode === 'correlate') {
  const run = process.argv[3];
  if (!run) throw new Error('usage: correlate <runId>');
  requireUuid('runId', run);
  const answer = await http('/query', [{ kinds: [46013, 46014, 46022, 46023], '#h': [channel], limit: 100 }]);
  if (answer.status !== 200) throw new Error('query refused: ' + JSON.stringify(answer));
  const rows = verifyAll(answer.body);
  writeJson('hook-signed-events.json', rows);
  const content = (e) => { try { return JSON.parse(e.content); } catch { return {}; } };
  const events = rows.filter((e) => (content(e).runId || content(e).result?.runId) === run);
  const request = events.find((x) => x.kind === 46013 && x.pubkey === relayPubkey);
  const claim = events.find((x) => x.kind === 46022);
  const result = events.find((x) => x.kind === 46023 && content(x).claimEventId === claim?.id);
  const echo = events.find((x) => x.kind === 46014 && content(x).result?.claimEventId === claim?.id);
  const summary = { mode, run, relayPubkey, witnessed, verifiedEventCount: rows.length, requestedEventId: request?.id ?? null, claimEventId: claim?.id ?? null, resultEventId: result?.id ?? null, relayEchoEventId: echo?.id ?? null, imports: 'nostr-tools only; no Beekeeper internals' };
  if (!request || !claim || !result || !echo) { console.log(JSON.stringify({ ...summary, complete: false })); process.exit(3); }
  if (echo.pubkey !== relayPubkey || content(echo).resultEventId !== result.id || content(echo).claimedBy !== claim.pubkey) throw new Error('relay echo admission correlation failed');
  if (content(claim).requestedEventId !== request.id || content(result).requestedEventId !== request.id) throw new Error('claim/result do not name the request');
  writeJson('hook-live-exchange.json', { witnessedRelayPubkey: relayPubkey, witnessed, acceptedClaimEventId: claim.id, request, claim, result, echo });
  console.log(JSON.stringify({ ...summary, complete: true }));
} else {
  throw new Error('mode must be correlate or claim');
}
