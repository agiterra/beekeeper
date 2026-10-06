import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import {
  CODING_SESSION_TITLE_SCHEMA,
  CODING_SESSION_TITLE_TAG_VERSION,
  MAX_CODING_SESSION_TITLE_CONTENT_BYTES,
  MAX_CODING_SESSION_TITLE_MODEL_BYTES,
  MAX_SESSION_NAME_BYTES,
  UNTITLED_SESSION_NAME,
  buildCodingSessionDisplayNameFilter,
  codingSessionTitleStanding,
  codingSessionTitleStandingQueries,
  foldCodingSessionNames,
  parseCodingSessionTitleParts,
  parseSessionTitleTargetKey,
  readCodingSessionTitleStanding,
  resolveSessionDisplayName,
} from "./codingSessionTitle.ts";
import { codingSessionNameKey } from "./codingSessionName.ts";

// The shared rule, byte for byte: the same file the Rust resolver binds to in
// `crates/beekeeper-core/src/coding_session_title_tests.rs`. Never edited to pass.
const HERE = path.dirname(fileURLToPath(import.meta.url));
const VECTORS_PATH = path.join(
  HERE,
  "../../../../../conformance/session-display-name/fixtures/vectors.json",
);
const corpus = JSON.parse(readFileSync(VECTORS_PATH, "utf8"));

test("the corpus pins the constants this build codes", () => {
  assert.equal(corpus.schema, "buzz.conformance/session-display-name@1");
  assert.deepEqual(corpus.constants, {
    kindGeneratedTitle: 44252,
    kindName: 44229,
    maxContentBytes: MAX_CODING_SESSION_TITLE_CONTENT_BYTES,
    maxModelBytes: MAX_CODING_SESSION_TITLE_MODEL_BYTES,
    maxTitleBytes: MAX_SESSION_NAME_BYTES,
    payloadSchema: CODING_SESSION_TITLE_SCHEMA,
    tagVersion: CODING_SESSION_TITLE_TAG_VERSION,
    untitled: UNTITLED_SESSION_NAME,
  });
  assert.ok(corpus.envelopes.length > 0);
  assert.ok(corpus.vectors.length > 0);
});

for (const envelope of corpus.envelopes) {
  test(`envelope: ${envelope.name}`, () => {
    const parsed = parseCodingSessionTitleParts(
      envelope.event.tags,
      envelope.event.content,
    );
    assert.equal(parsed !== null, envelope.valid, envelope.description);
  });
}

for (const vector of corpus.vectors) {
  test(`vector: ${vector.name}`, () => {
    const forwards = resolveSessionDisplayName(vector.scope, vector.events);
    assert.deepEqual(forwards, vector.expected, vector.description);
    const reversed = resolveSessionDisplayName(
      vector.scope,
      [...vector.events].reverse(),
    );
    assert.deepEqual(
      reversed,
      vector.expected,
      `${vector.description} (reversed)`,
    );
  });
}

test("a target key decodes only when it re-encodes to itself", () => {
  const key =
    "coding-session/v1|16:claude-agent-acp16:1958c6c448e05eed6:sess-a1:1";
  assert.deepEqual(parseSessionTitleTargetKey(key), {
    driver: "claude-agent-acp",
    instanceId: "1958c6c448e05eed",
    sessionId: "sess-a",
    generation: 1,
  });
  // A zero-padded generation decodes but does not re-encode to itself.
  assert.equal(parseSessionTitleTargetKey(key.replace(/1:1$/, "2:01")), null);
  assert.equal(parseSessionTitleTargetKey(`${key}x`), null);
  assert.equal(
    parseSessionTitleTargetKey(
      key.replace("coding-session/v1", "coding-session/v2"),
    ),
    null,
  );
});

test("filters always name their kinds", () => {
  assert.deepEqual(buildCodingSessionDisplayNameFilter(["c", "c"], 10), {
    kinds: [44229, 44252],
    "#h": ["c"],
    limit: 10,
  });
});

// ── Signed events through the desktop fold ──────────────────────────────────

const CHANNEL = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const SESSION = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const FOUNDER = generateSecretKey();
const PROVIDER = generateSecretKey();
const STRANGER = generateSecretKey();
const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "inst-1",
  sessionId: "sess-1",
  generation: 1,
};

function targetKey(target) {
  return `coding-session/v1|${[
    target.driver,
    target.instanceId,
    target.sessionId,
    String(target.generation),
  ]
    .map((field) => `${new TextEncoder().encode(field).byteLength}:${field}`)
    .join("")}`;
}

function titleEvent(secret, title, createdAt, target = TARGET) {
  return finalizeEvent(
    {
      kind: 44252,
      created_at: createdAt,
      tags: [
        ["h", CHANNEL],
        ["d", SESSION],
        ["cstl-v", "cstl1-1"],
        ["cs-target", targetKey(target)],
      ],
      content: JSON.stringify({
        schema: "buzz-coding-session-title/v1",
        title,
        model: "claude-haiku-4-5",
        basis: "first-message",
        sourceCommand: null,
        createEventId: "ca".repeat(32),
      }),
    },
    secret,
  );
}

function nameEvent(secret, content, createdAt) {
  return finalizeEvent(
    {
      kind: 44229,
      created_at: createdAt,
      tags: [
        ["h", CHANNEL],
        ["d", SESSION],
        ["csnm-v", "csnm1-1"],
      ],
      content,
    },
    secret,
  );
}

async function metadataEvent(
  secret,
  target = TARGET,
  sessionRef = SESSION,
  createdAt = 50,
) {
  const { codingSessionMetadataSemanticKey } = await import(
    "./codingSessionIngressPayloads.ts"
  );
  return finalizeEvent(
    {
      kind: 44223,
      created_at: createdAt,
      tags: [
        ["h", CHANNEL],
        ["csm-v", "csm1-1"],
        ["cs-target", targetKey(target)],
        ["csm-key", codingSessionMetadataSemanticKey(target)],
      ],
      content: JSON.stringify({
        schema: "buzz-coding-session-metadata/v1",
        session: target,
        projectRef: null,
        repoRef: null,
        title: "Fix the login redirect",
        agentRef: null,
        provider: "claude",
        runtime: "claude",
        model: null,
        status: "running",
        branch: null,
        capabilities: {
          threadTurnStart: true,
          threadTurnInterrupt: true,
          threadSteer: true,
          context: false,
          diff: true,
          plan: true,
        },
        sessionRef,
      }),
    },
    secret,
  );
}

async function receiptEvent(
  secret,
  {
    target = TARGET,
    status = "created",
    channel = CHANNEL,
    createdAt = 40,
  } = {},
) {
  const {
    CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
    codingSessionReceiptSemanticKey,
  } = await import("./codingSessionIngressPayloads.ts");
  const commandId = `cmd-${status}`;
  return finalizeEvent(
    {
      kind: 44224,
      created_at: createdAt,
      tags: [
        ["h", channel],
        ["cslr-v", "cslr1-1"],
        ["csl-command", commandId],
        ["csl-key", codingSessionReceiptSemanticKey(commandId, status)],
      ],
      content: JSON.stringify({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId,
        status,
        session: target,
        error: null,
      }),
    },
    secret,
  );
}

/** A signer's confirmed execution: its 44223 and a `created` receipt. */
async function confirmedExecution(secret, target = TARGET) {
  return [
    await metadataEvent(secret, target),
    await receiptEvent(secret, { target }),
  ];
}

test("standing comes from the signer's own 44223 for that target and sessionRef", async () => {
  const metadata = await metadataEvent(PROVIDER);
  const receipt = await receiptEvent(PROVIDER);
  const standing = codingSessionTitleStanding([metadata, receipt]);
  assert.deepEqual(standing.get(`${CHANNEL}\u0000${SESSION}`), [
    {
      targetKey: targetKey(TARGET),
      providerAuthorityPubkey: getPublicKey(PROVIDER),
    },
  ]);
  // Another sessionRef in the payload vouches for that session only.
  const elsewhere = await metadataEvent(
    PROVIDER,
    TARGET,
    "11111111-2222-4333-8444-555555555555",
  );
  assert.equal(
    codingSessionTitleStanding([elsewhere, receipt]).get(
      `${CHANNEL}\u0000${SESSION}`,
    ),
    undefined,
  );
  // A tampered signature vouches for nothing.
  assert.equal(
    codingSessionTitleStanding([{ ...metadata, sig: "00".repeat(64) }, receipt])
      .size,
    0,
  );
  assert.equal(
    codingSessionTitleStanding([metadata, { ...receipt, sig: "00".repeat(64) }])
      .size,
    0,
  );
});

test("a 44223 without a lifecycle receipt from its signer for that generation grants nothing", async () => {
  const metadata = await metadataEvent(PROVIDER);
  assert.equal(codingSessionTitleStanding([metadata]).size, 0);
  // Another signer's receipt confirms that signer, not this one.
  assert.equal(
    codingSessionTitleStanding([metadata, await receiptEvent(STRANGER)]).size,
    0,
  );
  // A receipt for another generation confirms that generation only.
  assert.equal(
    codingSessionTitleStanding([
      metadata,
      await receiptEvent(PROVIDER, { target: { ...TARGET, generation: 2 } }),
    ]).size,
    0,
  );
  // A receipt in another channel confirms nothing here.
  assert.equal(
    codingSessionTitleStanding([
      metadata,
      await receiptEvent(PROVIDER, {
        channel: "11111111-2222-4333-8444-555555555555",
      }),
    ]).size,
    0,
  );
  // A turn receipt names a generation but never confirms one (NIP-CSL).
  assert.equal(
    codingSessionTitleStanding([
      metadata,
      await receiptEvent(PROVIDER, { status: "turn_queued" }),
    ]).size,
    0,
  );
  // Any generation outcome does: resumed confirms as created does.
  assert.equal(
    codingSessionTitleStanding([
      metadata,
      await receiptEvent(PROVIDER, { status: "resumed" }),
    ]).size,
    1,
  );
});

test("a channel member with no execution cannot auto-name a session (bee sessions parity)", async () => {
  // The stranger signs a 44223 claiming SESSION with a cs-target of its own,
  // then a 44252 for it — but no lifecycle receipt ever confirms that
  // execution. `bee sessions show` prints nameOrigin: fallback and
  // foreignTitles: 1 for this; desktop must agree.
  const strangerTarget = { ...TARGET, instanceId: "inst-stranger" };
  const claim = await metadataEvent(STRANGER, strangerTarget);
  const title = titleEvent(STRANGER, "Hijacked title", 200, strangerTarget);
  const folded = foldCodingSessionNames({ events: [title], metadata: [claim] });
  const key = codingSessionNameKey(CHANNEL, SESSION, getPublicKey(FOUNDER));
  assert.equal(folded.names.get(key), undefined);
  assert.equal(folded.generatedTitles.size, 0);
  assert.deepEqual(folded.titleDiagnostics, { foreignTitles: 1, malformed: 0 });

  // Alongside a confirmed provider, the provider's title wins and the
  // stranger's is still counted, never shown.
  const withProvider = foldCodingSessionNames({
    events: [title, titleEvent(PROVIDER, "Login redirect fix", 300)],
    metadata: [claim, ...(await confirmedExecution(PROVIDER))],
  });
  assert.equal(withProvider.names.get(key)?.content, "Login redirect fix");
  assert.equal(withProvider.names.get(key)?.origin, "generated");
  assert.deepEqual(withProvider.titleDiagnostics, {
    foreignTitles: 1,
    malformed: 0,
  });
});

test("the fold resolves per founder key, and personNames never holds a title", async () => {
  const metadata = await confirmedExecution(PROVIDER);
  const title = titleEvent(PROVIDER, "Login redirect fix", 200);
  const key = codingSessionNameKey(CHANNEL, SESSION, getPublicKey(FOUNDER));
  const generatedOnly = foldCodingSessionNames({
    events: [title],
    metadata,
  });
  assert.deepEqual(generatedOnly.names.get(key), {
    channelId: CHANNEL,
    sessionRef: SESSION,
    founderPubkey: getPublicKey(FOUNDER),
    content: "Login redirect fix",
    createdAt: 200,
    eventId: title.id,
    origin: "generated",
    model: "claude-haiku-4-5",
    signerPubkey: getPublicKey(PROVIDER),
  });
  assert.equal(generatedOnly.personNames.size, 0);
  assert.equal(generatedOnly.names.size, 0, "iteration lists 44229s only");
  assert.equal(
    generatedOnly.generatedTitles.get(`${CHANNEL}\u0000${SESSION}`)?.content,
    "Login redirect fix",
  );
  assert.deepEqual(codingSessionTitleStandingQueries([title]), [
    {
      channelId: CHANNEL,
      signerPubkey: getPublicKey(PROVIDER),
      targetKeys: [targetKey(TARGET)],
      until: 260,
    },
  ]);

  const named = foldCodingSessionNames({
    events: [title, nameEvent(FOUNDER, "Auth rework", 100)],
    metadata,
  });
  assert.equal(named.names.get(key)?.content, "Auth rework");
  assert.equal(named.names.get(key)?.origin, "person");
  assert.equal(named.personNames.get(key)?.content, "Auth rework");

  // Without the provider's metadata the title has no standing: counted, unshown.
  const unvouched = foldCodingSessionNames({ events: [title], metadata: [] });
  assert.equal(unvouched.names.get(key), undefined);
  assert.equal(unvouched.names.has(key), false);
  assert.deepEqual(unvouched.titleDiagnostics, {
    foreignTitles: 1,
    malformed: 0,
  });

  // A stranger's 44229 is no person's name under the founder's key.
  const foreign = foldCodingSessionNames({
    events: [title, nameEvent(STRANGER, "Hijack", 300)],
    metadata,
  });
  assert.equal(foreign.names.get(key)?.content, "Login redirect fix");
  assert.equal(foreign.personNames.get(key), undefined);
});

// ── Standing is bounded by the claim, not by the signer's history ──────────

/** A relay over `events`: kinds, authors, `#h`, `until`, newest first, `limit`. */
function fakeRelay(events) {
  const filters = [];
  return {
    filters,
    fetch: async (filter) => {
      filters.push(filter);
      return events
        .filter(
          (event) =>
            filter.kinds.includes(event.kind) &&
            (!filter.authors || filter.authors.includes(event.pubkey)) &&
            (!filter["#h"] ||
              event.tags.some(
                (tag) => tag[0] === "h" && filter["#h"].includes(tag[1]),
              )) &&
            (filter.until === undefined || event.created_at <= filter.until),
        )
        .sort((left, right) => right.created_at - left.created_at)
        .slice(0, filter.limit);
    },
  };
}

let noiseCounter = 0;
/** A provider record that fills a page and vouches for nothing. */
function noise(secret, kind, createdAt) {
  noiseCounter += 1;
  const other = { ...TARGET, sessionId: `other-${noiseCounter}` };
  return {
    id: noiseCounter.toString(16).padStart(64, "0"),
    pubkey: getPublicKey(secret),
    kind,
    created_at: createdAt,
    tags:
      kind === 44224
        ? [
            ["h", CHANNEL],
            ["cslr-v", "cslr1-1"],
            ["csl-command", `turn-${noiseCounter}`],
            ["csl-key", "k"],
          ]
        : [
            ["h", CHANNEL],
            ["csm-v", "csm1-1"],
            ["cs-target", targetKey(other)],
            ["csm-key", "k"],
          ],
    content: "{}",
    sig: "00".repeat(64),
  };
}

test("1000+ newer turn receipts and metadata re-signs do not unseat an old title", async () => {
  const title = titleEvent(PROVIDER, "Login redirect fix", 200);
  const later = [];
  for (let index = 0; index < 1_500; index += 1) {
    later.push(noise(PROVIDER, 44224, 10_000 + index));
    later.push(noise(PROVIDER, 44223, 10_000 + index));
  }
  const relay = fakeRelay([...(await confirmedExecution(PROVIDER)), ...later]);
  const queries = codingSessionTitleStandingQueries([title]);
  const read = await readCodingSessionTitleStanding(queries, relay.fetch);
  assert.equal(read.incomplete, false);
  // One page per kind: the read starts at the title, not at the newest record.
  assert.equal(relay.filters.length, 2);
  for (const filter of relay.filters) {
    assert.deepEqual(filter["#h"], [CHANNEL]);
    assert.deepEqual(filter.authors, [getPublicKey(PROVIDER)]);
    assert.equal(filter.until, 260);
    assert.equal(filter.limit, 1000);
  }
  const folded = foldCodingSessionNames({
    events: [title],
    metadata: read.events,
  });
  assert.equal(
    folded.generatedTitles.get(`${CHANNEL}\u0000${SESSION}`)?.content,
    "Login redirect fix",
  );
  assert.deepEqual(folded.titleDiagnostics, { foreignTitles: 0, malformed: 0 });
});

test("standing older than a full page is found by paging backwards", async () => {
  const title = titleEvent(PROVIDER, "Login redirect fix", 5_000);
  const between = [];
  for (let index = 0; index < 25; index += 1) {
    between.push(noise(PROVIDER, 44224, 100 + index));
  }
  const relay = fakeRelay([
    ...(await confirmedExecution(PROVIDER)),
    ...between,
  ]);
  const read = await readCodingSessionTitleStanding(
    codingSessionTitleStandingQueries([title]),
    relay.fetch,
    { pageLimit: 10 },
  );
  assert.equal(read.incomplete, false);
  const receiptUntils = relay.filters
    .filter((filter) => filter.kinds[0] === 44224)
    .map((filter) => filter.until);
  assert.deepEqual(receiptUntils, [5_060, 115, 106]);
  assert.equal(
    foldCodingSessionNames({ events: [title], metadata: read.events })
      .generatedTitles.size,
    1,
  );
});

test("a claim whose history runs out is foreign; one cut short by the page cap says so", async () => {
  const title = titleEvent(PROVIDER, "Login redirect fix", 5_000);
  // Metadata but no receipt, and the history ends: a foreign title, read in full.
  const exhausted = await readCodingSessionTitleStanding(
    codingSessionTitleStandingQueries([title]),
    fakeRelay([await metadataEvent(PROVIDER)]).fetch,
  );
  assert.equal(exhausted.incomplete, false);
  const between = [];
  for (let index = 0; index < 30; index += 1) {
    between.push(noise(PROVIDER, 44224, 100 + index));
  }
  const capped = await readCodingSessionTitleStanding(
    codingSessionTitleStandingQueries([title]),
    fakeRelay([...(await confirmedExecution(PROVIDER)), ...between]).fetch,
    { pageLimit: 10, maxPages: 2 },
  );
  assert.equal(capped.incomplete, true);
  assert.equal(
    foldCodingSessionNames({ events: [title], metadata: capped.events })
      .generatedTitles.size,
    0,
  );
});
