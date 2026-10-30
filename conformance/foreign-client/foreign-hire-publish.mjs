// Fixture-only publication with independent Nostr signing. Loopback enforced.
// Input is either a complete signed event (exact replay) or a new event template.
import fs from 'node:fs';
import { loadKey, finalizeEvent, verifyEvent, nip98Client, relayHttpUrl, writeJson } from './_common.mjs';
const key = loadKey();
const input = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
const event = input.sig ? input : finalizeEvent({ ...input, created_at: input.created_at ?? Math.floor(Date.now() / 1000) }, key);
if (!verifyEvent(event)) throw new Error('invalid event to publish');
const response = await nip98Client(key, relayHttpUrl())('/events', event);
writeJson(process.argv[3], { event, response });
if (response.status !== 200 || response.body.accepted !== true) throw new Error(JSON.stringify(response));
console.log(JSON.stringify({event_id:event.id}));
