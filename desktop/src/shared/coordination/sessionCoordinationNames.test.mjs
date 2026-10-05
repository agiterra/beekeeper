import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { foldSessionCoordination } from "./sessionCoordinationFold.ts";
import {
  decodeSessionGenesisSessionRef,
  indexSessionGeneses,
  indexSessionNameRecords,
  resolveCoordinatedSessionName,
} from "./sessionCoordinationNames.ts";

// One accepted session from the Pulse corpus: create signed by the founder
// (a1…), receipt, metadata and lease by its provider (d4…).
const HERE = path.dirname(fileURLToPath(import.meta.url));
const corpus = JSON.parse(
  readFileSync(
    path.join(
      HERE,
      "../../../../conformance/project-pulse-fold/fixtures/fold-vectors.json",
    ),
    "utf8",
  ),
);
const CORPUS_BASE = corpus.vectors.find(
  (vector) => vector.name === "idle-hours-old-with-live-authorized-lease",
).input;
const CHANNEL = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const OTHER_CHANNEL = "6a1f0c3e-2b4d-5e6f-8a9b-0c1d2e3f4a5b";
const SESSION = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const FOUNDER = "a1".repeat(32);
const PROVIDER = "d4".repeat(32);
const STRANGER = "e5".repeat(32);
const TARGET = "coding-session/v1|3:acp6:inst-16:sess-11:1";
const GENESIS_ID = "9e".repeat(32);

/** A 44226 founding `sessionRef` in `channel`, signed by `pubkey`. */
function genesis(
  pubkey,
  { id = GENESIS_ID, channel = CHANNEL, sessionRef = SESSION } = {},
) {
  return {
    id,
    pubkey,
    created_at: 1_785_589_000,
    kind: 44226,
    tags: [
      ["h", channel],
      ["csg-v", "csg1-1"],
      ["csg-session", sessionRef],
    ],
    content: JSON.stringify({ sessionRef, v: 1 }),
  };
}

/** The corpus vector, its create naming `genesisRef` (null: as published). */
function baseEvents(genesisRef = GENESIS_ID) {
  return CORPUS_BASE.events.map((event) => {
    if (event.kind !== 44221 || genesisRef === null) return event;
    const content = JSON.parse(event.content);
    content.action.genesisRef = genesisRef;
    return { ...event, content: JSON.stringify(content) };
  });
}

function name(id, pubkey, content, createdAt) {
  return {
    id: id.repeat(64 / id.length),
    pubkey,
    created_at: createdAt,
    kind: 44229,
    tags: [
      ["h", CHANNEL],
      ["d", SESSION],
      ["csnm-v", "csnm1-1"],
    ],
    content,
  };
}

function title(id, pubkey, text, createdAt, target = TARGET) {
  return {
    id: id.repeat(64 / id.length),
    pubkey,
    created_at: createdAt,
    kind: 44252,
    tags: [
      ["h", CHANNEL],
      ["d", SESSION],
      ["cstl-v", "cstl1-1"],
      ["cs-target", target],
    ],
    content: JSON.stringify({
      schema: "buzz-coding-session-title/v1",
      title: text,
      model: "claude-haiku-4-5",
      basis: "first-message",
      sourceCommand: null,
      createEventId: "ca".repeat(32),
    }),
  };
}

function fold(extra, { genesisRef = GENESIS_ID, founder = FOUNDER } = {}) {
  return foldSessionCoordination({
    now: CORPUS_BASE.now,
    events: [
      ...baseEvents(genesisRef),
      ...(founder ? [genesis(founder)] : []),
      ...extra,
    ],
  });
}

test("Pulse keeps the founder's name over a newer stranger's and over any title", () => {
  const founderName = name("01", FOUNDER, "Auth rework", 1_785_590_100);
  const strangerName = name("02", STRANGER, "Hijacked", 1_785_590_900);
  const generated = title("03", PROVIDER, "Login redirect fix", 1_785_590_500);
  const result = fold([founderName, strangerName, generated]);
  const [session] = result.sessions;
  assert.equal(session.name, "Auth rework");
  assert.ok(session.sourceEventIds.includes(founderName.id));
  assert.ok(!session.sourceEventIds.includes(strangerName.id));
  assert.deepEqual(result.nameOriginsBySession.get(session.sessionKey), {
    origin: "person",
    model: null,
    signerPubkey: null,
  });
});

test("a foreign 44229 alone no longer names a Pulse session", () => {
  const result = fold([name("02", STRANGER, "Hijacked", 1_785_590_900)]);
  assert.equal(result.sessions[0].name, null);
  assert.equal(result.nameOriginsBySession.size, 0);
});

test("Pulse shows the earliest title from the session's own provider, and says it was generated", () => {
  const generated = title("03", PROVIDER, "Login redirect fix", 1_785_590_500);
  const later = title("04", PROVIDER, "Something else", 1_785_590_600);
  const stranger = title("05", STRANGER, "Stranger title", 1_785_590_000);
  const result = fold([later, stranger, generated]);
  const [session] = result.sessions;
  assert.equal(session.name, "Login redirect fix");
  assert.ok(session.sourceEventIds.includes(generated.id));
  assert.ok(!session.sourceEventIds.includes(stranger.id));
  assert.deepEqual(result.nameOriginsBySession.get(session.sessionKey), {
    origin: "generated",
    model: "claude-haiku-4-5",
    signerPubkey: PROVIDER,
  });
});

test("a title for a target the session does not hold is not its name", () => {
  const elsewhere = title(
    "06",
    PROVIDER,
    "Elsewhere",
    1_785_590_500,
    "coding-session/v1|3:acp6:inst-16:sess-91:1",
  );
  assert.equal(fold([elsewhere]).sessions[0].name, null);
});

test("the founder is the genesis signer, not the create's: a delegate's create names the founder's genesis", () => {
  // The corpus create is signed by a1…; here the genesis it names is signed
  // by e5…, so e5… is the founder and a1…'s name is a stranger's.
  const result = fold(
    [
      name("01", FOUNDER, "Create signer's", 1_785_590_100),
      name("02", STRANGER, "Genesis signer's", 1_785_590_050),
    ],
    { founder: STRANGER },
  );
  assert.equal(result.sessions[0].name, "Genesis signer's");
});

test("no genesis in the read, or a create naming none, proves no founder", () => {
  const founderName = name("01", FOUNDER, "Auth rework", 1_785_590_100);
  const generated = title("03", PROVIDER, "Login redirect fix", 1_785_590_500);
  for (const options of [{ founder: null }, { genesisRef: null }]) {
    const result = fold([founderName, generated], options);
    // The person tier is empty, so the provider's title shows — and says so.
    assert.equal(result.sessions[0].name, "Login redirect fix");
    assert.equal(
      result.nameOriginsBySession.get(result.sessions[0].sessionKey)?.origin,
      "generated",
    );
  }
});

test("a genesis in another channel or for another session proves nothing", () => {
  const index = indexSessionNameRecords([name("01", FOUNDER, "Founder's", 10)]);
  const witnesses = [
    {
      targetKey: TARGET,
      providerAuthorityPubkey: PROVIDER,
      genesisRef: GENESIS_ID,
    },
  ];
  const resolve = (event) =>
    resolveCoordinatedSessionName({
      channelId: CHANNEL,
      sessionRef: SESSION,
      witnesses,
      geneses: indexSessionGeneses([event]),
      index,
    });
  assert.equal(resolve(genesis(FOUNDER))?.name, "Founder's");
  assert.equal(resolve(genesis(FOUNDER, { channel: OTHER_CHANNEL })), null);
  assert.equal(
    resolve(
      genesis(FOUNDER, { sessionRef: "0b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10" }),
    ),
    null,
  );
});

test("two creates proving different founders leave the founder unknown", () => {
  const second = "8f".repeat(32);
  const index = indexSessionNameRecords([name("01", FOUNDER, "Founder's", 10)]);
  const geneses = indexSessionGeneses([
    genesis(FOUNDER),
    genesis(STRANGER, { id: second }),
  ]);
  const witness = (genesisRef) => ({
    targetKey: TARGET,
    providerAuthorityPubkey: PROVIDER,
    genesisRef,
  });
  const resolve = (witnesses) =>
    resolveCoordinatedSessionName({
      channelId: CHANNEL,
      sessionRef: SESSION,
      witnesses,
      geneses,
      index,
    });
  assert.equal(
    resolve([witness(GENESIS_ID), witness(GENESIS_ID)])?.name,
    "Founder's",
  );
  assert.equal(resolve([witness(GENESIS_ID), witness(second)]), null);
  assert.equal(resolve([]), null, "no accepted create → no founder");
});

test("the genesis decoder is the Rust one's exact two forms", () => {
  const adopts = {
    createEventId: "ab".repeat(32),
    receiptEventId: "cd".repeat(32),
  };
  assert.equal(
    decodeSessionGenesisSessionRef(
      JSON.stringify({ sessionRef: SESSION, v: 1 }),
    ),
    SESSION,
  );
  assert.equal(
    decodeSessionGenesisSessionRef(
      JSON.stringify({ sessionRef: SESSION, v: 1, adopts }),
    ),
    SESSION,
  );
  for (const bad of [
    JSON.stringify({ sessionRef: SESSION, v: 2 }),
    JSON.stringify({ sessionRef: SESSION, v: 1, adopts: null }),
    JSON.stringify({
      sessionRef: SESSION,
      v: 1,
      adopts: { ...adopts, extra: 1 },
    }),
    JSON.stringify({
      sessionRef: SESSION,
      v: 1,
      adopts: { createEventId: adopts.createEventId },
    }),
    JSON.stringify({ sessionRef: SESSION, v: 1, extra: true }),
    JSON.stringify({ sessionRef: SESSION.toUpperCase(), v: 1 }),
    `{"sessionRef":"${SESSION}","v":1.0}`,
    `{"sessionRef":"${SESSION}","sessionRef":"${SESSION}","v":1}`,
    "not json",
  ]) {
    assert.equal(decodeSessionGenesisSessionRef(bad), null, bad);
  }
});
