// Shared plumbing for the foreign protocol clients in this directory.
//
// Imports: Node built-ins and `nostr-tools` only. Nothing from the Beekeeper
// SDK, the host, the desktop, or any Beekeeper file layout — the point of
// these scripts is to prove the wire contract is reachable without them.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

// `nostr-tools` is resolved from FOREIGN_CLIENT_NOSTR_TOOLS (a directory or a
// package.json whose node_modules holds it), else from this repository's
// desktop install. Any install works; it is the only dependency.
function resolveNostrTools() {
  const here = path.dirname(fileURLToPath(import.meta.url));
  const configured = process.env.FOREIGN_CLIENT_NOSTR_TOOLS;
  const anchor = configured
    ? (fs.statSync(configured).isDirectory() ? path.join(configured, 'package.json') : configured)
    : path.join(here, '..', '..', 'desktop', 'package.json');
  try {
    return createRequire(anchor)('nostr-tools');
  } catch (error) {
    throw new Error(`nostr-tools not found via ${anchor} (set FOREIGN_CLIENT_NOSTR_TOOLS): ${error.message}`);
  }
}

export const nostr = resolveNostrTools();
export const { finalizeEvent, verifyEvent, generateSecretKey, getPublicKey } = nostr;

const LOOPBACK = new Set(['127.0.0.1', '::1', '[::1]', 'localhost']);

// The relay to talk to, as an http(s) origin. RELAY_URL may be ws(s):// or
// http(s)://. Anything but loopback is refused unless the caller sets
// FOREIGN_CLIENT_ALLOW_REMOTE=1 — the same safety the original scripts had
// with their hardcoded 127.0.0.1:3037, without the hardcoding.
export function relayHttpUrl() {
  const raw = process.env.RELAY_URL || 'http://127.0.0.1:3037';
  const url = new URL(raw);
  if (url.protocol === 'ws:') url.protocol = 'http:';
  if (url.protocol === 'wss:') url.protocol = 'https:';
  if (!['http:', 'https:'].includes(url.protocol)) throw new Error(`RELAY_URL must be http(s):// or ws(s)://, got ${raw}`);
  if (!LOOPBACK.has(url.hostname) && process.env.FOREIGN_CLIENT_ALLOW_REMOTE !== '1') {
    throw new Error(`refusing non-loopback relay ${url.hostname}; set FOREIGN_CLIENT_ALLOW_REMOTE=1 to override`);
  }
  return url.origin;
}

// A 32-byte secret key from, in order: FOREIGN_CLIENT_KEY (hex),
// FOREIGN_CLIENT_KEY_FILE (an env file with BEEKEEPER_PRIVATE_KEY=<hex>),
// BEEKEEPER_PRIVATE_KEY (hex). Never printed.
export function loadKey() {
  let hex = process.env.FOREIGN_CLIENT_KEY;
  if (!hex && process.env.FOREIGN_CLIENT_KEY_FILE) {
    const line = fs.readFileSync(process.env.FOREIGN_CLIENT_KEY_FILE, 'utf8').split('\n')
      .find((x) => x.startsWith('BEEKEEPER_PRIVATE_KEY='));
    if (!line) throw new Error('key file has no BEEKEEPER_PRIVATE_KEY= line');
    hex = line.slice(line.indexOf('=') + 1).trim();
  }
  if (!hex) hex = process.env.BEEKEEPER_PRIVATE_KEY;
  if (!hex || !/^[0-9a-f]{64}$/.test(hex)) throw new Error('no 64-hex key in FOREIGN_CLIENT_KEY, FOREIGN_CLIENT_KEY_FILE or BEEKEEPER_PRIVATE_KEY');
  return Uint8Array.from(Buffer.from(hex, 'hex'));
}

export function requireEnv(name) {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is required`);
  return value;
}

export function requireUuid(name, value) {
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(value)) throw new Error(`${name} must be a lowercase UUID`);
  return value;
}

// Where result files go. Defaults to the working directory.
export function outPath(name) {
  const dir = process.env.FOREIGN_CLIENT_OUT || '.';
  fs.mkdirSync(dir, { recursive: true });
  return path.join(dir, name);
}

export function writeJson(name, value) {
  const target = outPath(name);
  fs.writeFileSync(target, JSON.stringify(value, null, 2) + '\n');
  return target;
}

// NIP-98 authenticated HTTP against the relay's generic Nostr bridge
// (`POST /events`, `POST /query`, `POST /count`). Returns `{status, body}`
// for every HTTP answer, so a caller can assert on a refusal; throws only on
// a transport failure or a non-JSON body.
export function nip98Client(key, origin) {
  return async function http(route, body, method = 'POST') {
    const url = origin + route;
    const text = body === undefined ? undefined : JSON.stringify(body);
    const tags = [['u', url], ['method', method]];
    if (text !== undefined) tags.push(['payload', crypto.createHash('sha256').update(text).digest('hex')]);
    const auth = finalizeEvent({ kind: 27235, created_at: Math.floor(Date.now() / 1000), content: '', tags }, key);
    const response = await fetch(url, {
      method,
      headers: { Authorization: 'Nostr ' + Buffer.from(JSON.stringify(auth)).toString('base64'), 'Content-Type': 'application/json' },
      body: text,
    });
    const raw = await response.text();
    let parsed;
    try { parsed = JSON.parse(raw); } catch { parsed = { raw }; }
    return { status: response.status, body: parsed };
  };
}

// Every event in `rows` must carry a valid signature; a relay answer that
// fails this is not evidence of anything.
export function verifyAll(rows) {
  if (!Array.isArray(rows)) throw new Error('expected an array of events, got ' + JSON.stringify(rows).slice(0, 200));
  for (const event of rows) if (!verifyEvent(event)) throw new Error('invalid event signature ' + event.id);
  return rows;
}

// The NIP-CSC exact-target key (`cs-target` tag), byte-for-byte what
// `beekeeper_sdk::builders::coding_session_target_key` builds.
export function targetKey(target) {
  return 'coding-session/v1|' + [target.driver, target.instanceId, target.sessionId, String(target.generation)]
    .map((x) => Buffer.byteLength(x) + ':' + x).join('');
}

export function now() { return Math.floor(Date.now() / 1000); }
