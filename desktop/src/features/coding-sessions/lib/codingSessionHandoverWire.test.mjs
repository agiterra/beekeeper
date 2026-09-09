import assert from "node:assert/strict";
import test from "node:test";

import {
  buildCodingSessionHandoverContent,
  buildCodingSessionHandoverTags,
  CODING_SESSION_HANDOVER_SCHEMA,
  decodeCodingSessionHandoverEvent,
  KIND_CODING_SESSION_HANDOVER,
} from "./codingSessionHandoverWire.ts";

// Lane K's Rust-written fixture is the eventual pin for this suite. It had not
// landed when these tests were written, so every event here is built literally
// against `docs/HANDOVER_IMPL.md` §2 — the shapes, not a second reading of
// them. When `codingSessionHandover.fixture.json` appears, its events replace
// these builders and a Rust-side rename becomes a failing test here.
const CHANNEL = "11111111-2222-4333-8444-555555555555";
const SESSION = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
const GENESIS = "a1".repeat(32);
const AUTHOR = "b2".repeat(32);
const EVENT_ID = "c3".repeat(32);
const SHA = "9".repeat(40);

function checkpointBody(overrides = {}) {
  return {
    prevCheckpointRef: null,
    task: "Land the handover panel",
    assignmentRefs: ["d4".repeat(32)],
    decisions: [
      { eventId: "e5".repeat(32), summary: "Reconstruct, not native" },
    ],
    revision: {
      repoRef: `30617:${"f6".repeat(32)}:beekeeper`,
      baseSha: SHA,
      headSha: SHA,
      branch: "work/handover",
      dirty: true,
      preserved: "partial",
    },
    artifacts: [
      {
        kind: "wip-ref",
        repoRef: `30617:${"f6".repeat(32)}:beekeeper`,
        ref: "refs/heads/wip/builder/abc12345",
        sha: SHA,
      },
      {
        kind: "patch",
        repoRef: `30617:${"f6".repeat(32)}:beekeeper`,
        eventId: "a7".repeat(32),
        baseSha: SHA,
        bytes: 4096,
      },
      {
        kind: "blob",
        repoRef: `30617:${"f6".repeat(32)}:beekeeper`,
        hash: "b8".repeat(32),
        baseSha: SHA,
        bytes: 131072,
      },
    ],
    tests: [{ name: "desktop unit", command: "pnpm test", outcome: "passed" }],
    unresolved: ["Does the fence cover sibling executions?"],
    nextAction: "Wire the panel into the workspace",
    missing: ["editor scratch buffer"],
    ...overrides,
  };
}

function continuationBody(overrides = {}) {
  return {
    claimRef: "c9".repeat(32),
    mode: "reconstructed",
    checkpointRef: EVENT_ID,
    target: {
      driver: "acp",
      instanceId: "instance-b",
      sessionId: "session-b",
      generation: 2,
    },
    recovered: [`wip-ref refs/heads/wip/builder/abc12345 at ${SHA}`],
    missing: ["uncommitted changes on A's machine"],
    note: "Reconstructed on B's provider",
    ...overrides,
  };
}

function event(type, body, overrides = {}) {
  return {
    id: EVENT_ID,
    pubkey: AUTHOR,
    created_at: 1_700_000_000,
    kind: KIND_CODING_SESSION_HANDOVER,
    tags: buildCodingSessionHandoverTags({
      channelId: CHANNEL,
      sessionRef: SESSION,
      genesisRef: GENESIS,
      type,
    }),
    content: buildCodingSessionHandoverContent({
      sessionRef: SESSION,
      genesisRef: GENESIS,
      type,
      body,
    }),
    sig: "0".repeat(128),
    ...overrides,
  };
}

function decode(candidate) {
  return decodeCodingSessionHandoverEvent({
    event: candidate,
    channelRef: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
  });
}

test("a checkpoint this app builds is a checkpoint this app reads", () => {
  const decoded = decode(event("checkpoint", checkpointBody()));
  assert.equal(decoded.ok, true);
  assert.equal(decoded.value.type, "checkpoint");
  assert.equal(decoded.value.eventId, EVENT_ID);
  assert.equal(decoded.value.author, AUTHOR);
  assert.equal(decoded.value.createdAt, 1_700_000_000);
  assert.equal(decoded.value.sessionRef, SESSION);
  assert.equal(decoded.value.genesisRef, GENESIS);
  assert.equal(decoded.value.body.revision.preserved, "partial");
  assert.equal(decoded.value.body.revision.dirty, true);
  assert.deepEqual(
    decoded.value.body.artifacts.map((artifact) => artifact.kind),
    ["wip-ref", "patch", "blob"],
  );
  assert.deepEqual(decoded.value.body.missing, ["editor scratch buffer"]);
});

test("a continuation carries the claim it acted under and both outcome words", () => {
  const decoded = decode(event("continuation", continuationBody()));
  assert.equal(decoded.ok, true);
  assert.equal(decoded.value.type, "continuation");
  assert.equal(decoded.value.body.mode, "reconstructed");
  assert.equal(decoded.value.body.claimRef, "c9".repeat(32));
  assert.equal(decoded.value.body.target.generation, 2);

  const native = decode(
    event("continuation", continuationBody({ mode: "native-resume" })),
  );
  assert.equal(native.ok, true);
  assert.equal(native.value.body.mode, "native-resume");
});

test("`prevCheckpointRef` is always present, and null is a real answer", () => {
  const first = decode(event("checkpoint", checkpointBody()));
  assert.equal(first.ok, true);
  assert.equal(first.value.body.prevCheckpointRef, null);

  const next = decode(
    event("checkpoint", checkpointBody({ prevCheckpointRef: EVENT_ID })),
  );
  assert.equal(next.ok, true);
  assert.equal(next.value.body.prevCheckpointRef, EVENT_ID);

  // Absent is refused by name — a checkpoint whose author never stated what it
  // replaces is a different record from one this build read loosely.
  const body = checkpointBody();
  delete body.prevCheckpointRef;
  const absent = decode(event("checkpoint", body));
  assert.equal(absent.ok, false);
  assert.match(absent.reason, /checkpoint is missing "prevCheckpointRef"/);

  const malformed = decode(
    event("checkpoint", checkpointBody({ prevCheckpointRef: "not-hex" })),
  );
  assert.equal(malformed.ok, false);
  assert.match(
    malformed.reason,
    /"prevCheckpointRef" must be a lowercase 64-hex id/,
  );
});

test("an absent `preserved` is refused by name rather than read as `all`", () => {
  const body = checkpointBody();
  delete body.revision.preserved;
  const decoded = decode(event("checkpoint", body));
  assert.equal(decoded.ok, false);
  assert.match(decoded.reason, /checkpoint\.revision is missing "preserved"/);
});

test("an unknown `preserved` word is refused rather than coerced", () => {
  const decoded = decode(
    event(
      "checkpoint",
      checkpointBody({
        revision: { ...checkpointBody().revision, preserved: "most" },
      }),
    ),
  );
  assert.equal(decoded.ok, false);
  assert.match(
    decoded.reason,
    /"preserved" must be one of "all", "partial", "none"/,
  );
});

test("one extra key anywhere in the body is named, not ignored", () => {
  const decoded = decode(
    event("checkpoint", { ...checkpointBody(), summary: "extra" }),
  );
  assert.equal(decoded.ok, false);
  assert.match(decoded.reason, /checkpoint carries unsupported "summary"/);
});

test("an unknown artifact kind is refused", () => {
  const decoded = decode(
    event(
      "checkpoint",
      checkpointBody({
        artifacts: [{ kind: "stash", repoRef: "r", ref: "x", sha: SHA }],
      }),
    ),
  );
  assert.equal(decoded.ok, false);
  assert.match(decoded.reason, /"kind" must be "wip-ref", "patch" or "blob"/);
});

test("an unknown continuation mode is refused", () => {
  const decoded = decode(
    event("continuation", continuationBody({ mode: "teleported" })),
  );
  assert.equal(decoded.ok, false);
  assert.match(
    decoded.reason,
    /"mode" must be one of "native-resume", "reconstructed"/,
  );
});

test("hex widths are enforced on every reference", () => {
  const short = decode(
    event("continuation", continuationBody({ claimRef: "abc" })),
  );
  assert.equal(short.ok, false);
  assert.match(short.reason, /"claimRef" must be a lowercase 64-hex id/);

  const badSha = decode(
    event(
      "checkpoint",
      checkpointBody({
        revision: { ...checkpointBody().revision, headSha: "zz".repeat(20) },
      }),
    ),
  );
  assert.equal(badSha.ok, false);
  assert.match(badSha.reason, /"headSha" must be a 40- or 64-hex commit sha/);
});

test("bounds are enforced: sixteen assignments, not seventeen", () => {
  const decoded = decode(
    event(
      "checkpoint",
      checkpointBody({
        assignmentRefs: Array.from({ length: 17 }, (_unused, index) =>
          index.toString(16).padStart(2, "0").repeat(32),
        ),
      }),
    ),
  );
  assert.equal(decoded.ok, false);
  assert.match(decoded.reason, /at most 16 lowercase 64-hex ids/);
});

test("a task over 4 KiB is refused", () => {
  const decoded = decode(
    event("checkpoint", checkpointBody({ task: "x".repeat(4097) })),
  );
  assert.equal(decoded.ok, false);
  assert.match(
    decoded.reason,
    /"task" must be a non-empty string of at most 4096 bytes/,
  );
});

test("an envelope that crosses scope is refused, however well formed", () => {
  const other = "ffffffff-eeee-4ddd-8ccc-bbbbbbbbbbbb";
  const decoded = decodeCodingSessionHandoverEvent({
    event: event("checkpoint", checkpointBody()),
    channelRef: CHANNEL,
    sessionRef: other,
    genesisRef: GENESIS,
  });
  assert.equal(decoded.ok, false);
  assert.match(decoded.reason, /crosses or disagrees with its supplied scope/);
});

test("the csh-type tag must agree with the payload type", () => {
  const mismatched = event("checkpoint", checkpointBody());
  mismatched.tags = buildCodingSessionHandoverTags({
    channelId: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
    type: "continuation",
  });
  const decoded = decode(mismatched);
  assert.equal(decoded.ok, false);
  assert.match(decoded.reason, /"csh-type" disagrees with the payload type/);
});

test("a sixth tag, a wrong version, or a wrong kind is refused", () => {
  const extraTag = event("checkpoint", checkpointBody());
  extraTag.tags = [...extraTag.tags, ["p", AUTHOR]];
  assert.equal(decode(extraTag).ok, false);

  const wrongVersion = event("checkpoint", checkpointBody());
  wrongVersion.tags[2] = ["csh-v", "csh2"];
  assert.match(decode(wrongVersion).reason, /"csh-v" must be "csh1"/);

  const wrongKind = event("checkpoint", checkpointBody(), { kind: 44246 });
  assert.match(decode(wrongKind).reason, /kind 44246 is not 44247/);
});

test("content over 32 KiB, duplicate keys, and non-JSON are each refused", () => {
  const huge = event("checkpoint", checkpointBody());
  huge.content = JSON.stringify({
    schema: CODING_SESSION_HANDOVER_SCHEMA,
    sessionRef: SESSION,
    genesisRef: GENESIS,
    type: "checkpoint",
    body: checkpointBody({ task: "x".repeat(40_000) }),
  });
  assert.match(decode(huge).reason, /content exceeds 32768 bytes/);

  const duplicate = event("checkpoint", checkpointBody());
  duplicate.content = '{"type":"checkpoint","type":"continuation"}';
  assert.match(decode(duplicate).reason, /repeats a JSON key/);

  const garbage = event("checkpoint", checkpointBody());
  garbage.content = "not json";
  assert.match(decode(garbage).reason, /content is not JSON/);
});
