// Relay-only control of an exact public target: steer (kind:44220) or a
// lifecycle action (kind:44221). Never opens executor state or a host socket.
//
//   node foreign-native-control.mjs <steer|stop|resume|restart> '<spec json>'
//   spec: {operation, channel, target:{driver,instanceId,sessionId,generation},
//          provider, text (steer only), output}
import { finalizeEvent, loadKey, nip98Client, now, relayHttpUrl, requireUuid, targetKey, writeJson } from './_common.mjs';

const key = loadKey();
const http = nip98Client(key, relayHttpUrl());
const action = process.argv[2];
const spec = JSON.parse(process.argv[3] || 'null');
if (!action || !spec?.operation || !spec?.channel || !spec?.target || !spec?.provider || !spec?.output) throw new Error('explicit public target required: {operation, channel, target, provider, output}');
requireUuid('spec.channel', spec.channel);
const turn = action === 'steer';
if (!turn && !['stop', 'resume', 'restart'].includes(action)) throw new Error('action must be steer, stop, resume or restart');
const payload = turn
  ? { schema: 'buzz-coding-session-command/v1', commandId: spec.operation, target: spec.target, action: { type: 'thread.turn.start', text: spec.text, deliver: 'steer' } }
  : { schema: 'buzz-coding-session-lifecycle-command/v1', commandId: spec.operation, action: { type: 'session.' + action, session: spec.target, providerAuthorityPubkey: spec.provider } };
const tags = turn
  ? [['h', spec.channel], ['cs-v', 'csc1-1'], ['cs-target', targetKey(spec.target)]]
  : [['h', spec.channel], ['csl-v', 'csl1-1'], ['csl-command', spec.operation]];
const event = finalizeEvent({ kind: turn ? 44220 : 44221, created_at: now(), tags, content: JSON.stringify(payload) }, key);
const result = { event, response: await http('/events', event) };
writeJson(spec.output, result);
console.log(JSON.stringify({ action, operation: spec.operation, eventId: event.id, status: result.response.status, response: result.response.body }));
