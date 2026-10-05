/**
 * SV-31 through the whole read path: signed events in, the store, the
 * umbrella fold, and the display name, origin and attribution out. The rule
 * itself is pinned by the shared vectors (`sessionTitle.test.mjs`); this file
 * proves the web fold feeds it the right founder and executions.
 */
import assert from "node:assert/strict";
import { test } from "node:test";

import { CodingSessionObserverStore } from "./catalog.ts";
import {
  CHANNEL_ID,
  createEvent,
  generatedTitleEvent,
  genesisEvent,
  metadataEvent,
  nameEvent,
  newSigner,
  receiptEvent,
  SESSION_REF,
  sign,
  target,
} from "./testFixtures.mjs";
import {
  buildCodingSessionObserverSnapshot,
  codingSessionTitleOriginDetail,
} from "./umbrella.ts";
import {
  codingSessionGeneratedTitlesFilter,
  codingSessionHistoryFilters,
} from "./filters.ts";

const channels = [CHANNEL_ID];

function snapshotOf(events) {
  const store = new CodingSessionObserverStore();
  store.ingest(events, channels);
  return buildCodingSessionObserverSnapshot(store.facts(channels), {
    nowMs: 1_700_000_000_000,
    leasesRead: false,
  });
}

/** A governed session: genesis, a create, its receipt, and metadata. */
function governed(founder, provider, options = {}) {
  const executionTarget = options.target ?? target();
  const genesis = genesisEvent(founder, { sessionRef: SESSION_REF });
  return [
    genesis,
    createEvent(founder, {
      providerAuthorityPubkey: provider.pubkey,
      sessionRef: SESSION_REF,
      genesisRef: genesis.id,
    }),
    receiptEvent(provider, { status: "created", target: executionTarget }),
    metadataEvent(provider, {
      target: executionTarget,
      sessionRef: SESSION_REF,
      runtime: "claude-agent-acp",
      model: "opus",
      title: options.metadataTitle,
    }),
  ];
}

test("a provider's title names a session nobody named, and says so", () => {
  const founder = newSigner();
  const provider = newSigner();
  const [umbrella] = snapshotOf([
    ...governed(founder, provider),
    generatedTitleEvent(provider, { title: "Login redirect fix" }),
  ]).umbrellas;
  assert.equal(umbrella.name, "Login redirect fix");
  assert.equal(umbrella.nameOrigin, "generated");
  assert.equal(umbrella.nameModel, "claude-haiku-4-5");
  assert.equal(umbrella.nameSigner, provider.pubkey);
  assert.equal(
    codingSessionTitleOriginDetail(umbrella),
    `Named automatically from the first message by Claude Code · opus (${provider.pubkey.slice(0, 8)}…${provider.pubkey.slice(-4)}) · claude-haiku-4-5`,
  );
});

test("the founder's rename wins over the generated title, however old", () => {
  const founder = newSigner();
  const provider = newSigner();
  const [umbrella] = snapshotOf([
    ...governed(founder, provider),
    nameEvent(founder, { name: "Auth rework", created_at: 1 }),
    generatedTitleEvent(provider, { title: "Login redirect fix" }),
  ]).umbrellas;
  assert.equal(umbrella.name, "Auth rework");
  assert.equal(umbrella.nameOrigin, "person");
  assert.equal(umbrella.nameModel, null);
  assert.equal(umbrella.nameSigner, null);
  assert.equal(codingSessionTitleOriginDetail(umbrella), null);
});

test("a 44229 from someone other than the founder is nobody's name", () => {
  const founder = newSigner();
  const provider = newSigner();
  const stranger = newSigner();
  const [umbrella] = snapshotOf([
    ...governed(founder, provider),
    nameEvent(stranger, { name: "Hijacked", created_at: 9_999_999_999 }),
  ]).umbrellas;
  // The fixture's metadata title ("A session") is the founding title.
  assert.equal(umbrella.name, "A session");
  assert.equal(umbrella.nameOrigin, "fallback");
  assert.equal(umbrella.nameDiagnostics.foreignNames, 1);
});

test("a title from a signer with no execution here falls back, counted", () => {
  const founder = newSigner();
  const provider = newSigner();
  const stranger = newSigner();
  const [umbrella] = snapshotOf([
    ...governed(founder, provider, { metadataTitle: "Fix the parser" }),
    generatedTitleEvent(stranger, { title: "Not yours", created_at: 1 }),
  ]).umbrellas;
  assert.equal(umbrella.name, "Fix the parser");
  assert.equal(umbrella.nameOrigin, "fallback");
  assert.equal(umbrella.nameDiagnostics.foreignTitles, 1);
  assert.equal(codingSessionTitleOriginDetail(umbrella), null);
});

test("the earliest standing title wins, so a shown title never flips", () => {
  const founder = newSigner();
  const provider = newSigner();
  const [umbrella] = snapshotOf([
    ...governed(founder, provider),
    generatedTitleEvent(provider, { title: "Later", created_at: 20 }),
    generatedTitleEvent(provider, { title: "Earlier", created_at: 10 }),
  ]).umbrellas;
  assert.equal(umbrella.name, "Earlier");
});

test("a malformed 44252 is counted by the store, never shown", () => {
  const founder = newSigner();
  const provider = newSigner();
  const malformed = sign(provider, {
    kind: 44252,
    content: "{not json",
    tags: [
      ["h", CHANNEL_ID],
      ["d", SESSION_REF],
      ["cstl-v", "cstl1-1"],
      ["cs-target", "coding-session/v1|wrong"],
    ],
  });
  const snapshot = snapshotOf([...governed(founder, provider), malformed]);
  assert.equal(snapshot.umbrellas[0].nameOrigin, "fallback");
  assert.equal(snapshot.malformedCount, 1);
});

test("generated titles are read on their own page", () => {
  assert.deepEqual(
    codingSessionGeneratedTitlesFilter(CHANNEL_ID).kinds,
    [44252],
  );
  const kinds = codingSessionHistoryFilters(CHANNEL_ID).map(
    (filter) => filter.kinds,
  );
  assert.ok(kinds.some((list) => list.length === 1 && list[0] === 44252));
  assert.ok(kinds.some((list) => list.length === 1 && list[0] === 44229));
});

test("a title from an unverified (D5 fallback) authority is foreign", () => {
  // No readable create: the first-seen metadata signer stands in as the
  // disclosed fallback, which anyone in the channel can be. Its title must
  // not name the session — desktop, mobile and `bee` all require proof.
  const provider = newSigner();
  const [umbrella] = snapshotOf([
    receiptEvent(provider, { status: "created" }),
    metadataEvent(provider, {
      sessionRef: SESSION_REF,
      title: "Fix the parser",
    }),
    generatedTitleEvent(provider, { title: "Self-appointed" }),
  ]).umbrellas;
  assert.equal(umbrella.executions[0].authoritySource, "disclosed-fallback");
  assert.equal(umbrella.name, "Fix the parser");
  assert.equal(umbrella.nameOrigin, "fallback");
  assert.equal(umbrella.nameDiagnostics.foreignTitles, 1);
});

test("the same title stands once the create it names is readable", () => {
  const founder = newSigner();
  const provider = newSigner();
  const [umbrella] = snapshotOf([
    createEvent(founder, {
      providerAuthorityPubkey: provider.pubkey,
      sessionRef: SESSION_REF,
    }),
    receiptEvent(provider, { status: "created" }),
    metadataEvent(provider, { sessionRef: SESSION_REF }),
    generatedTitleEvent(provider, { title: "Proven" }),
  ]).umbrellas;
  assert.equal(umbrella.executions[0].authoritySource, "create");
  assert.equal(umbrella.name, "Proven");
  assert.equal(umbrella.nameOrigin, "generated");
});
