import assert from "node:assert/strict";
import test from "node:test";

import {
  classifyCodingSessionCheckpointEvent,
  codingSessionCheckpointSemanticKey,
  decodeCodingSessionCheckpoint,
  foldCodingSessionCheckpoints,
  isPublishableCheckpointPath,
  previousCodingSessionCheckpoint,
} from "./codingSessionCheckpoints.ts";
import {
  CHANNEL_ID,
  checkpointEvent,
  gitFacts,
  payload,
  PROVIDER_PUBKEY,
  scope,
  STRANGER_SECRET,
  TARGET,
  TARGET_KEY,
  TREE_A,
  TREE_B,
  TREE_C,
  tagsFor,
  unavailablePayload,
} from "./codingSessionCheckpointFixtures.testFixtures.mjs";

const decode = (body) => decodeCodingSessionCheckpoint(JSON.stringify(body));

test("decodes the NIP-CSCK example shapes", () => {
  const turn = decode(payload());
  assert.equal(turn.ok, true);
  assert.equal(turn.value.files[0].path, "src/main.rs");
  assert.equal(decode(unavailablePayload()).ok, true);
  assert.equal(
    decode(payload({ reason: "pre_rewind", turnId: null })).ok,
    true,
  );
  const renamed = decode(
    payload({
      files: [
        {
          path: "b.txt",
          status: "renamed",
          from: "a.txt",
          additions: null,
          deletions: null,
        },
      ],
    }),
  );
  assert.equal(renamed.ok, true);
  assert.equal(renamed.value.files[0].additions, null);
});

test("the semantic key mirrors the NIP example: reason is part of identity", () => {
  const key = codingSessionCheckpointSemanticKey(TARGET, "turn", 58);
  assert.ok(key.startsWith("coding-session-checkpoint/v1|16:claude-agent-acp"));
  assert.ok(key.endsWith("1:22:584:turn"));
  assert.ok(
    codingSessionCheckpointSemanticKey(TARGET, "pre_rewind", 58).endsWith(
      "2:5810:pre_rewind",
    ),
  );
});

test("unknown keys are rejected at every level", () => {
  const cases = [
    payload({ extra: 1 }),
    payload({ session: { ...TARGET, extra: 1 } }),
    payload({ coverage: { fromSeq: 1, throughSeq: 2, extra: 1 } }),
    payload({ git: { ...gitFacts(), extra: 1 } }),
    payload({
      git: gitFacts({
        complete: false,
        omitted: [{ path: "big.bin", reason: "too_large", extra: 1 }],
      }),
    }),
    payload({
      files: [
        {
          path: "a",
          status: "added",
          from: null,
          additions: 1,
          deletions: 0,
          extra: 1,
        },
      ],
    }),
    unavailablePayload("TIMED_OUT", {
      unavailable: { code: "TIMED_OUT", sentence: "Timed out.", extra: 1 },
    }),
  ];
  for (const body of cases) {
    assert.equal(decode(body).ok, false, JSON.stringify(body));
  }
});

test("absent is not null, and null only where nullable", () => {
  const withoutSummary = payload();
  delete withoutSummary.summary;
  assert.equal(decode(withoutSummary).ok, false);
  const withoutHead = payload({ git: gitFacts() });
  delete withoutHead.git.head;
  assert.equal(decode(withoutHead).ok, false);
  assert.equal(decode(payload({ git: gitFacts({ tree: null }) })).ok, false);
  assert.equal(decode(payload({ filesNotListed: null })).ok, false);
  assert.equal(
    decode(
      payload({
        git: gitFacts({
          head: null,
          branch: null,
          baseTree: null,
          outsideTurn: null,
        }),
      }),
    ).ok,
    true,
  );
});

test("cross-field rules: one of git/unavailable, turnId, coverage, summary", () => {
  assert.equal(
    decode(payload({ unavailable: { code: "GIT_FAILED", sentence: "x" } })).ok,
    false,
  );
  assert.equal(decode(payload({ git: null })).ok, false);
  assert.equal(decode(payload({ turnId: null })).ok, false);
  assert.equal(
    decode(payload({ coverage: { fromSeq: 9, throughSeq: 8 } })).ok,
    false,
  );
  assert.equal(
    decode(payload({ coverage: { fromSeq: 0, throughSeq: 8 } })).ok,
    false,
  );
  assert.equal(decode(payload({ summary: "context" })).ok, false);
  assert.equal(
    decode(payload({ schema: "buzz-coding-session-checkpoint/v2" })).ok,
    false,
  );
  assert.equal(decode(payload({ reason: "rewind" })).ok, false);
  assert.equal(
    decode(unavailablePayload("NOT_A_REPOSITORY", { filesNotListed: 1 })).ok,
    false,
  );
  assert.equal(decode(unavailablePayload("NOPE")).ok, false);
  assert.equal(
    decode(payload({ session: { ...TARGET, generation: 0 } })).ok,
    false,
  );
  assert.equal(
    decode(payload({ session: { ...TARGET, driver: "a\nb" } })).ok,
    false,
  );
  assert.equal(decode(payload({ turnId: "   " })).ok, false);
  assert.equal(decode(payload({ restorable: "no" })).ok, false);
});

test("git facts: oids, widths, branch, omissions", () => {
  assert.equal(
    decode(payload({ git: gitFacts({ tree: "A".repeat(40) }) })).ok,
    false,
  );
  assert.equal(
    decode(payload({ git: gitFacts({ commit: "d".repeat(64) }) })).ok,
    false,
  );
  assert.equal(
    decode(payload({ git: gitFacts({ branch: "refs/heads/main" }) })).ok,
    false,
  );
  assert.equal(
    decode(
      payload({
        git: gitFacts({ omitted: [{ path: "big.bin", reason: "too_large" }] }),
      }),
    ).ok,
    false,
    "complete must be false when a path is omitted",
  );
  assert.equal(
    decode(payload({ git: gitFacts({ omittedNotListed: 2 }) })).ok,
    false,
  );
  assert.equal(
    decode(
      payload({
        git: gitFacts({
          complete: false,
          omitted: [
            { path: "a", reason: "too_large" },
            { path: "a", reason: "unreadable" },
          ],
        }),
      }),
    ).ok,
    false,
  );
  assert.equal(
    decode(payload({ git: gitFacts({ outsideTurn: "yes" }) })).ok,
    false,
  );
});

test("files: status, renames, counts, duplicates, caps", () => {
  const file = (over) => ({
    path: "a",
    status: "added",
    from: null,
    additions: 1,
    deletions: 0,
    ...over,
  });
  assert.equal(
    decode(payload({ files: [file({ status: "copied" })] })).ok,
    false,
  );
  assert.equal(
    decode(payload({ files: [file({ status: "renamed" })] })).ok,
    false,
  );
  assert.equal(decode(payload({ files: [file({ from: "b" })] })).ok, false);
  assert.equal(
    decode(payload({ files: [file({ status: "renamed", from: "a" })] })).ok,
    false,
  );
  assert.equal(decode(payload({ files: [file({ additions: -1 })] })).ok, false);
  assert.equal(
    decode(payload({ files: [file({ additions: 1.5 })] })).ok,
    false,
  );
  assert.equal(decode(payload({ files: [file(), file()] })).ok, false);
  const many = Array.from({ length: 257 }, (_, index) =>
    file({ path: `f${index}` }),
  );
  assert.equal(decode(payload({ files: many })).ok, false);
});

test("paths are repo-relative and canonical, never host paths or markers", () => {
  for (const bad of [
    "/etc/passwd",
    "\\share",
    "C:/x",
    "a/../b",
    "./a",
    "a//b",
    "a\\..\\b",
    "a\u0000b",
    "refs/heads/main",
    "secret/[redacted]",
    "x/••••••••",
    "[elided private context: 3]",
    "",
    "a".repeat(1025),
  ]) {
    assert.equal(isPublishableCheckpointPath(bad), false, JSON.stringify(bad));
    assert.equal(
      decode(
        payload({
          files: [
            {
              path: bad,
              status: "added",
              from: null,
              additions: 0,
              deletions: 0,
            },
          ],
        }),
      ).ok,
      false,
    );
  }
  assert.equal(isPublishableCheckpointPath("src/a b/c.ts"), true);
});

test("raw bytes: duplicate keys, oversize, and non-JSON are refused", () => {
  const text = JSON.stringify(payload());
  assert.equal(
    decodeCodingSessionCheckpoint(
      text.replace(
        '"restorable":false',
        '"restorable":false,"restorable":true',
      ),
    ).ok,
    false,
  );
  assert.equal(decodeCodingSessionCheckpoint("{").ok, false);
  assert.equal(
    decodeCodingSessionCheckpoint(`${text}${" ".repeat(33 * 1024)}`).ok,
    false,
  );
});

test("envelope: five ordered tags, each re-derived from the content", () => {
  const ok = classifyCodingSessionCheckpointEvent(checkpointEvent(), scope());
  assert.equal(ok.kind, "checkpoint");
  assert.equal(ok.entry.targetKey, TARGET_KEY);
  assert.equal(ok.entry.signerPubkey, PROVIDER_PUBKEY);

  const body = payload();
  const tags = tagsFor(body);
  const variants = [
    tags.slice(0, 4),
    [...tags, ["x", "y"]],
    [tags[1], tags[0], ...tags.slice(2)],
    tags.map((tag, index) => (index === 3 ? ["csck-seq", "57"] : tag)),
    tags.map((tag, index) => (index === 1 ? ["csck-v", "csck1-2"] : tag)),
    tags.map((tag, index) =>
      index === 4
        ? [
            "csck-key",
            codingSessionCheckpointSemanticKey(TARGET, "pre_rewind", 58),
          ]
        : tag,
    ),
    tags.map((tag, index) =>
      index === 0 ? ["h", CHANNEL_ID.toUpperCase()] : tag,
    ),
  ];
  for (const variant of variants) {
    const event = checkpointEvent(body, { tags: variant });
    assert.equal(
      classifyCodingSessionCheckpointEvent(event, scope()).kind,
      "malformed",
      JSON.stringify(variant),
    );
  }
  assert.equal(
    classifyCodingSessionCheckpointEvent(
      checkpointEvent(body, { kind: 44225 }),
      scope(),
    ).kind,
    "irrelevant",
  );
});

test("signer: a checkpoint not signed by the target's 44225 key is refused", () => {
  const foreign = checkpointEvent(payload(), { secret: STRANGER_SECRET });
  assert.equal(
    classifyCodingSessionCheckpointEvent(foreign, scope()).kind,
    "foreign-signer",
  );
  const other = payload({ session: { ...TARGET, generation: 9 } });
  assert.equal(
    classifyCodingSessionCheckpointEvent(checkpointEvent(other), scope()).kind,
    "unknown-target",
  );
  const forged = { ...checkpointEvent(), sig: "0".repeat(128) };
  assert.equal(
    classifyCodingSessionCheckpointEvent(forged, scope()).kind,
    "invalid-signature",
  );
});

test("fold: one per turn, highest throughSeq wins, duplicates and pre_rewind apart", () => {
  const first = checkpointEvent(
    payload({ coverage: { fromSeq: 1, throughSeq: 10 } }),
  );
  const later = checkpointEvent(
    payload({
      coverage: { fromSeq: 1, throughSeq: 12 },
      git: gitFacts({ tree: TREE_C }),
    }),
  );
  const duplicate = checkpointEvent(
    payload({
      coverage: { fromSeq: 1, throughSeq: 12 },
      git: gitFacts({ tree: TREE_A }),
    }),
  );
  const second = checkpointEvent(
    payload({ turnId: "turn-2", coverage: { fromSeq: 13, throughSeq: 20 } }),
  );
  const rewind = checkpointEvent(
    payload({
      reason: "pre_rewind",
      turnId: null,
      coverage: { fromSeq: 13, throughSeq: 20 },
    }),
  );
  const foreign = checkpointEvent(payload(), { secret: STRANGER_SECRET });
  const fold = foldCodingSessionCheckpoints(
    [second, later, duplicate, first, rewind, foreign, first],
    scope(),
  );
  assert.equal(fold.byScope.size, 1);
  const [generation] = fold.byScope.values();
  assert.deepEqual(
    generation.turns.map((entry) => entry.payload.coverage.throughSeq),
    [12, 20],
  );
  assert.equal(generation.byTurnId.get("turn-1").payload.git.tree, TREE_C);
  assert.equal(generation.preRewind.length, 1);
  assert.equal(fold.rejected.duplicate, 1);
  assert.equal(fold.rejected.foreignSigner, 1);
  assert.equal(
    previousCodingSessionCheckpoint(
      generation,
      generation.byTurnId.get("turn-2"),
    ),
    generation.byTurnId.get("turn-1"),
  );
  assert.equal(
    previousCodingSessionCheckpoint(generation, generation.turns[0]),
    null,
  );
  assert.equal(TREE_B.length, 40);
});
