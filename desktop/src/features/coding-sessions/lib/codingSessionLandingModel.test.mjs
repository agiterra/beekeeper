import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_LANDING_NO_REPOSITORY,
  CODING_SESSION_LANDING_NOT_READ,
  CODING_SESSION_LANDING_PROVENANCE_UNCHECKED_NOTE,
  CODING_SESSION_LANDING_RUNNING_TITLE,
  CODING_SESSION_LANDING_SIGNER_UNCHECKED,
  codingSessionLandingModel,
  codingSessionLandingNewestGateHead,
  codingSessionLandingRuleUnavailableReason,
  codingSessionLandingVerdictClass,
} from "./codingSessionLandingModel.ts";

const HEAD = "07c470be007c470be007c470be007c470be007c4";
const OLD = "aaaa1111aaaa1111aaaa1111aaaa1111aaaa1111";
const SEAT = "11".repeat(32);
const PROVIDER = "22".repeat(32);

function gate({
  gate = "cargo test",
  outcome = "passed",
  commitSha = HEAD,
  dirty = false,
  source = "observed",
  eventId,
  author = PROVIDER,
}) {
  return {
    key: `${author}:${gate}:${eventId}`,
    authorPubkey: author,
    source,
    gate,
    outcome,
    command: `${gate} --all`,
    summaryLines: [],
    hiddenSummaryLines: 0,
    duration: null,
    commitShortSha: commitSha === null ? null : commitSha.slice(0, 8),
    commitSha,
    dirty,
    sourceEventId: eventId,
    droppedEventIds: 0,
    assignmentUnresolved: false,
  };
}

const formatTime = (ms) => `T${ms}`;
const resolveWho = (pubkey) => (pubkey === SEAT ? "Bob" : pubkey.slice(0, 8));

function readGates(rows, times = {}) {
  return {
    state: "read",
    rows,
    signedAt: new Map(Object.entries(times)),
  };
}

const READY_LAND = {
  state: "ready",
  buttonLabel: `Land ${HEAD.slice(0, 7)} on main`,
  headSha: HEAD,
  command: `git push origin ${HEAD}:refs/heads/main`,
  approvalSentence: "ok",
  notRunSentence: "not run",
  sentence: null,
  foundersSentence: "Founders: you.",
  viewerIsFounder: true,
  repositorySourceNote: null,
  policyLine: null,
  bindingLine: null,
};

function base(overrides = {}) {
  return {
    gates: readGates([]),
    running: [],
    provenanceChecked: true,
    rule: { state: "read", land: READY_LAND, newestVerdict: null },
    main: { state: "read", commits: [HEAD], checkedAtMs: 5 },
    resolveWho,
    formatTime,
    ...overrides,
  };
}

test("Gate: the newest head is the most recently signed row naming a commit", () => {
  const rows = [
    gate({ eventId: "e1", commitSha: OLD }),
    gate({ eventId: "e2", commitSha: HEAD }),
    gate({ eventId: "e3", commitSha: null }),
  ];
  assert.equal(
    codingSessionLandingNewestGateHead(
      rows,
      new Map([
        ["e1", 10],
        ["e2", 20],
        ["e3", 30],
      ]),
    ),
    HEAD,
  );
  assert.equal(
    codingSessionLandingNewestGateHead(
      rows,
      new Map([
        ["e1", 40],
        ["e2", 20],
      ]),
    ),
    OLD,
  );
});

test("Gate: newest row per gate for the head, failures first, others disclosed", () => {
  const model = codingSessionLandingModel(
    base({
      gates: readGates(
        [
          gate({ eventId: "a", gate: "cargo test", outcome: "passed" }),
          gate({
            eventId: "b",
            gate: "cargo test",
            outcome: "failed",
            author: SEAT,
            source: "declared",
          }),
          gate({ eventId: "c", gate: "clippy", outcome: "passed" }),
          gate({
            eventId: "d",
            gate: "clippy",
            outcome: "failed",
            commitSha: OLD,
          }),
          gate({
            eventId: "e",
            gate: "fmt",
            outcome: "passed",
            commitSha: null,
          }),
        ],
        { a: 30, b: 20, c: 25, d: 10, e: 5 },
      ),
    }),
  );
  assert.equal(model.head?.sha, HEAD);
  assert.equal(model.head?.source, "gate");
  // A seat's declared failure and the provider's observed pass are different
  // authors: neither shadows the other (the fold's own key), and the failure
  // leads.
  assert.deepEqual(
    model.gate.rows.map((row) => `${row.gate}:${row.outcome}:${row.source}`),
    [
      "cargo test:failed:declared",
      "cargo test:passed:observed",
      "clippy:passed:observed",
    ],
  );
  assert.equal(model.gate.tone, "attention");
  assert.equal(model.gate.word, "failed");
  assert.equal(model.gate.sentence, "1 of 2 gates failed on 07c470be.");
  // The commitless row is listed in its own group, not reduced to a count.
  assert.deepEqual(
    model.gate.unnamedRows.map((row) => `${row.gate}:${row.outcome}`),
    ["fmt:passed"],
  );
  assert.deepEqual(model.gate.notes, [
    "1 gate row names an older commit and is not listed.",
  ]);
});

test("Gate: an observed failure is not shadowed by a newer declared pass on the head", () => {
  const model = codingSessionLandingModel(
    base({
      gates: readGates(
        [
          gate({ eventId: "o", gate: "just ci", outcome: "failed" }),
          gate({
            eventId: "d",
            gate: "just ci",
            outcome: "passed",
            author: SEAT,
            source: "declared",
          }),
        ],
        { o: 10, d: 20 },
      ),
    }),
  );
  assert.equal(model.gate.tone, "attention");
  assert.equal(model.gate.word, "failed");
  assert.equal(model.gate.sentence, "1 of 1 gate failed on 07c470be.");
  assert.deepEqual(
    model.gate.rows.map((row) => `${row.source}:${row.outcome}`),
    ["observed:failed", "declared:passed"],
  );
});

test("Gate: same author, same source, same gate — the newest wins and the rest are counted", () => {
  const model = codingSessionLandingModel(
    base({
      gates: readGates(
        [
          gate({ eventId: "a", outcome: "failed" }),
          gate({ eventId: "b", outcome: "passed" }),
        ],
        { a: 1, b: 2 },
      ),
    }),
  );
  assert.deepEqual(
    model.gate.rows.map((row) => row.outcome),
    ["passed"],
  );
  assert.deepEqual(model.gate.notes, [
    "1 older statement by the same author about the same gate is not listed.",
  ]);
});

test("Gate: an observed failure that names no commit reads failed, in attention", () => {
  const model = codingSessionLandingModel(
    base({
      gates: readGates(
        [gate({ eventId: "o", outcome: "failed", commitSha: null })],
        { o: 10 },
      ),
      rule: {
        state: "read",
        land: { ...READY_LAND, state: "ungoverned", headSha: null },
        newestVerdict: null,
      },
    }),
  );
  assert.equal(model.head, null);
  assert.equal(model.gate.tone, "attention");
  assert.equal(model.gate.word, "failed (no commit named)");
  assert.equal(
    model.gate.sentence,
    "cargo test failed in a row that names no commit.",
  );
  assert.deepEqual(
    model.gate.unnamedRows.map((row) => `${row.gate}:${row.outcome}`),
    ["cargo test:failed"],
  );
});

test("Gate: an observed failure with no commit beside a measured pass on a named head", () => {
  const model = codingSessionLandingModel(
    base({
      gates: readGates(
        [
          gate({ eventId: "m", source: "measured", outcome: "passed" }),
          gate({ eventId: "o", outcome: "failed", commitSha: null }),
        ],
        { m: 5, o: 10 },
      ),
    }),
  );
  assert.equal(model.head?.sha, HEAD);
  assert.equal(model.gate.tone, "attention");
  assert.equal(model.gate.word, "failed (no commit named)");
  assert.equal(
    model.gate.sentence,
    "cargo test failed in a row that names no commit; 1 of 1 gate passed on 07c470be.",
  );
  assert.equal(model.gate.rows.length, 1);
  assert.equal(model.gate.unnamedRows[0].outcome, "failed");
});

test("Gate: a pass over a dirty tree is said, and is not ok", () => {
  const model = codingSessionLandingModel(
    base({
      gates: readGates(
        [
          gate({ eventId: "a", gate: "cargo test", dirty: true }),
          gate({ eventId: "b", gate: "clippy" }),
        ],
        { a: 1, b: 2 },
      ),
    }),
  );
  assert.notEqual(model.gate.tone, "ok");
  assert.equal(model.gate.word, "passed (dirty tree)");
  assert.equal(
    model.gate.sentence,
    "2 gates passed on 07c470be, 1 over uncommitted changes, which is not evidence about that commit.",
  );
});

test("Gate: a failed gate on the head is attention, and leads the list", () => {
  const model = codingSessionLandingModel(
    base({
      gates: readGates(
        [
          gate({
            eventId: "a",
            gate: "clippy",
            outcome: "passed",
            dirty: true,
          }),
          gate({ eventId: "b", gate: "cargo test", outcome: "failed" }),
        ],
        { a: 1, b: 2 },
      ),
    }),
  );
  assert.equal(model.gate.tone, "attention");
  assert.equal(model.gate.word, "failed");
  assert.equal(model.gate.rows[0].gate, "cargo test");
  // The dirty mark travels with the row for the component to print.
  assert.equal(model.gate.rows[1].dirty, true);
  assert.equal(model.gate.sentence, "1 of 2 gates failed on 07c470be.");
});

test("Gate (SV-41): an open start reads running since T, a stale one no result", () => {
  const model = codingSessionLandingModel(
    base({
      gates: readGates([gate({ eventId: "a" })], { a: 1 }),
      running: [
        {
          key: "k1",
          gate: "cargo test",
          startedAtMs: 1000,
          authorPubkey: PROVIDER,
          eventId: "s1",
          stale: false,
        },
        {
          key: "k2",
          gate: "clippy",
          startedAtMs: 2000,
          authorPubkey: PROVIDER,
          eventId: "s2",
          stale: true,
        },
      ],
    }),
  );
  assert.equal(model.gate.tone, "running");
  assert.equal(model.gate.word, "running");
  assert.deepEqual(
    model.gate.running.map((line) => line.line),
    [
      "cargo test · running since T1000 · watched by 22222222",
      "clippy · started T2000 · no result observed · watched by 22222222",
    ],
  );
  assert.equal(
    model.gate.running[0].title,
    `Started T1000 by the provider's clock. ${CODING_SESSION_LANDING_RUNNING_TITLE}`,
  );
  assert.match(
    model.gate.running[1].title,
    /^Started T2000 by the provider's clock\./,
  );
  assert.equal(model.gate.running[0].eventId, "s1");
  assert.match(
    model.gate.sentence,
    /^1 gate is running; 1 of 1 finished gate passed on 07c470be\.$/,
  );
});

test("Gate (SV-41): a running line sits above a failed row, which keeps attention", () => {
  const model = codingSessionLandingModel(
    base({
      gates: readGates(
        [gate({ eventId: "b", gate: "cargo test", outcome: "failed" })],
        { b: 2 },
      ),
      running: [
        {
          key: "k1",
          gate: "cargo test",
          startedAtMs: 3000,
          authorPubkey: PROVIDER,
          eventId: "s1",
          stale: false,
        },
      ],
    }),
  );
  // A rerun in flight never hides the failure it may replace.
  assert.equal(model.gate.tone, "attention");
  assert.equal(model.gate.word, "failed");
  assert.equal(model.gate.rows[0].outcome, "failed");
  assert.deepEqual(
    model.gate.running.map((line) => line.line),
    ["cargo test · running since T3000 · watched by 22222222"],
  );
  assert.equal(model.gate.running[0].stale, false);
});

test("Gate: only stale starts do not read as running", () => {
  const model = codingSessionLandingModel(
    base({
      gates: readGates([]),
      running: [
        {
          key: "k",
          gate: "clippy",
          startedAtMs: 2000,
          authorPubkey: PROVIDER,
          eventId: "s",
          stale: true,
        },
      ],
    }),
  );
  assert.equal(model.gate.tone, "neutral");
  assert.equal(model.gate.word, "no gate rows");
});

test("Gate: no read is a muted 'not read', never red", () => {
  for (const gates of [
    {
      state: "not-read",
      reason: "This session has no genesis, so it has no observations.",
    },
    { state: "error", message: "relay closed" },
  ]) {
    const model = codingSessionLandingModel(base({ gates }));
    assert.equal(model.gate.tone, "unknown");
    assert.equal(model.gate.word, CODING_SESSION_LANDING_NOT_READ);
    assert.notEqual(model.gate.tone, "attention");
  }
  const loading = codingSessionLandingModel(
    base({ gates: { state: "loading" } }),
  );
  assert.equal(loading.gate.tone, "unknown");
});

test("Gate: rows naming no commit speak for no head", () => {
  const model = codingSessionLandingModel(
    base({
      gates: readGates([gate({ eventId: "a", commitSha: null })]),
      rule: {
        state: "read",
        land: { ...READY_LAND, state: "ungoverned", headSha: null },
        newestVerdict: null,
      },
    }),
  );
  assert.equal(model.head, null);
  assert.equal(model.gate.word, "no commit named");
  assert.equal(model.landed.word, "no head");
});

test("Verdict: the newest verdict, who signed it, and its tone", () => {
  assert.equal(codingSessionLandingVerdictClass("approve"), "clears");
  assert.equal(codingSessionLandingVerdictClass("not-refuted"), "clears");
  assert.equal(
    codingSessionLandingVerdictClass("changes-requested"),
    "refuses",
  );
  assert.equal(codingSessionLandingVerdictClass("refuted"), "refuses");
  assert.equal(codingSessionLandingVerdictClass("blocked"), "other");

  const refusing = codingSessionLandingModel(
    base({
      gates: readGates([gate({ eventId: "a" })], { a: 1 }),
      rule: {
        state: "read",
        land: READY_LAND,
        newestVerdict: {
          eventId: "v".repeat(64),
          authorPubkey: SEAT,
          decision: "changes-requested",
          reportEventId: "r".repeat(64),
          headSha: OLD,
        },
      },
    }),
  );
  assert.equal(refusing.verdict.tone, "attention");
  assert.equal(refusing.verdict.word, "changes-requested");
  assert.equal(
    refusing.verdict.sentence,
    "changes-requested by Bob, over report rrrrrrrr on aaaa1111. That is not the newest gated commit, 07c470be.",
  );

  const none = codingSessionLandingModel(base());
  assert.equal(none.verdict.word, "none");
  assert.equal(none.verdict.tone, "neutral");
});

test("Verdict and Land: an unasked or failed rule is not read, never red", () => {
  for (const rule of [
    codingSessionLandingRuleUnavailableReason("no-identity"),
    codingSessionLandingRuleUnavailableReason("boundary-failed"),
    { state: "asking" },
  ]) {
    const model = codingSessionLandingModel(base({ rule }));
    assert.equal(model.verdict.tone, "unknown");
    assert.equal(model.land.tone, "unknown");
    assert.equal(model.land.land, null);
    assert.ok(model.land.sentence);
  }
});

test("Land: ready, refused, refused over unread gate rows, ungoverned", () => {
  assert.equal(codingSessionLandingModel(base()).land.word, "ready");
  const refused = codingSessionLandingModel(
    base({
      rule: {
        state: "read",
        newestVerdict: null,
        land: {
          ...READY_LAND,
          state: "refused",
          headSha: null,
          sentence: "Not ready to land: no verifier cleared report 1234.",
        },
      },
    }),
  );
  assert.equal(refused.land.tone, "attention");
  assert.equal(refused.land.word, "refused");
  const unread = codingSessionLandingModel(
    base({
      rule: {
        state: "read",
        newestVerdict: null,
        land: {
          ...READY_LAND,
          state: "refused",
          headSha: null,
          sentence:
            "Not ready to land: x. This view read no gate rows for this mission, so ...",
        },
      },
    }),
  );
  assert.equal(unread.land.tone, "unknown");
  const ungoverned = codingSessionLandingModel(
    base({
      rule: {
        state: "read",
        newestVerdict: null,
        land: { ...READY_LAND, state: "ungoverned" },
      },
    }),
  );
  assert.equal(ungoverned.land.word, "not governed");
  assert.equal(ungoverned.land.tone, "neutral");
});

test("Landed: on main, stamped with when it was checked", () => {
  const tip = codingSessionLandingModel(
    base({ gates: readGates([gate({ eventId: "a" })], { a: 1 }) }),
  );
  assert.equal(tip.landed.word, "on main");
  assert.equal(tip.landed.tone, "ok");
  assert.equal(tip.landed.sentence, "07c470be is main's newest commit.");
  assert.equal(tip.landed.checkedAt, "checked at T5");

  const behind = codingSessionLandingModel(
    base({
      gates: readGates([gate({ eventId: "a" })], { a: 1 }),
      main: {
        state: "read",
        commits: [OLD, "b".repeat(40), HEAD],
        checkedAtMs: 9,
      },
    }),
  );
  assert.equal(
    behind.landed.sentence,
    "07c470be is on main, 2 commits behind its tip.",
  );
});

test("Landed: a head missing from bounded history is 'not seen in main's last N', never 'not landed'", () => {
  const model = codingSessionLandingModel(
    base({
      gates: readGates([gate({ eventId: "a" })], { a: 1 }),
      main: {
        state: "read",
        commits: Array.from({ length: 50 }, (_, i) =>
          String(i).padStart(40, "0"),
        ),
        checkedAtMs: 7,
      },
    }),
  );
  assert.equal(model.landed.word, "not seen");
  assert.equal(model.landed.tone, "neutral");
  assert.equal(
    model.landed.sentence,
    "07c470be is not seen in main's last 50 commits.",
  );
  assert.doesNotMatch(JSON.stringify(model), /not landed/i);
});

test("Landed: an unread history is not read, never red", () => {
  for (const main of [
    { state: "error", message: "clone refused" },
    {
      state: "not-read",
      reason: "The repository announcement names no clone URL.",
    },
    { state: "loading" },
  ]) {
    const model = codingSessionLandingModel(
      base({ gates: readGates([gate({ eventId: "a" })], { a: 1 }), main }),
    );
    assert.equal(model.landed.tone, "unknown");
    assert.equal(model.landed.checkedAt, null);
  }
});

test("Head falls back to the verdict's commit, then the land rule's", () => {
  const verdictHead = codingSessionLandingModel(
    base({
      rule: {
        state: "read",
        land: {
          ...READY_LAND,
          headSha: null,
          state: "refused",
          sentence: "Not ready to land: x",
        },
        newestVerdict: {
          eventId: "v",
          authorPubkey: SEAT,
          decision: "approve",
          reportEventId: "r".repeat(64),
          headSha: OLD,
        },
      },
    }),
  );
  assert.equal(verdictHead.head?.sha, OLD);
  assert.equal(verdictHead.head?.source, "verdict");
  const landHead = codingSessionLandingModel(base());
  assert.equal(landHead.head?.source, "land");
});

test("Gate: an unchecked fold says the signer was not checked, on the line and in a note", () => {
  const model = codingSessionLandingModel(
    base({
      provenanceChecked: false,
      gates: readGates([gate({ eventId: "a" })], { a: 1 }),
      running: [
        {
          key: "k1",
          gate: "cargo test",
          startedAtMs: 1000,
          authorPubkey: SEAT,
          eventId: "s1",
          stale: false,
        },
      ],
    }),
  );
  assert.equal(
    model.gate.running[0].line,
    "cargo test · running since T1000 · signed as observed by Bob",
  );
  assert.ok(
    model.gate.running[0].title.includes(
      CODING_SESSION_LANDING_SIGNER_UNCHECKED,
    ),
  );
  assert.doesNotMatch(model.gate.running[0].title, /provider's clock/);
  assert.ok(
    model.gate.notes.includes(CODING_SESSION_LANDING_PROVENANCE_UNCHECKED_NOTE),
  );
  const checked = codingSessionLandingModel(
    base({ gates: readGates([gate({ eventId: "a" })], { a: 1 }) }),
  );
  assert.ok(
    !checked.gate.notes.includes(
      CODING_SESSION_LANDING_PROVENANCE_UNCHECKED_NOTE,
    ),
  );
});

test("Landed: an empty history read says so, never 'last 0 commits'", () => {
  const model = codingSessionLandingModel(
    base({
      gates: readGates([gate({ eventId: "a" })], { a: 1 }),
      main: { state: "read", commits: [], checkedAtMs: 3 },
    }),
  );
  assert.equal(model.landed.tone, "unknown");
  assert.equal(model.landed.sentence, "main's history came back empty.");
  assert.doesNotMatch(JSON.stringify(model.landed), /last 0/);
});

test("Landed: a failed refresh over an older read is disclosed", () => {
  const model = codingSessionLandingModel(
    base({
      gates: readGates([gate({ eventId: "a" })], { a: 1 }),
      main: {
        state: "read",
        commits: [HEAD],
        checkedAtMs: 5,
        refreshError: "relay closed",
      },
    }),
  );
  assert.equal(model.landed.word, "on main");
  assert.equal(
    model.landed.sentence,
    "07c470be is main's newest commit. Checking again failed (relay closed); this is the read from T5.",
  );
});

test("Gate (SV-41): a running line read from a snapshot says it is not live", () => {
  const notLive = {
    short: "not live — read at 14:02",
    sentence: "Not live: the subscription is down.",
  };
  const model = codingSessionLandingModel(
    base({
      gates: readGates([gate({ eventId: "a" })], { a: 1 }),
      running: [
        {
          key: "k1",
          gate: "cargo test",
          startedAtMs: 1000,
          authorPubkey: PROVIDER,
          eventId: "s1",
          stale: false,
          notLive,
        },
      ],
    }),
  );
  assert.equal(
    model.gate.running[0].line,
    "cargo test · running since T1000 · watched by 22222222 · not live — read at 14:02",
  );
  assert.match(
    model.gate.running[0].title,
    /Not live: the subscription is down\.$/,
  );
});

test("No repository: the gate failure still shows; Land and Landed say there is nothing to land into", () => {
  const model = codingSessionLandingModel(
    base({
      gates: readGates([
        gate({
          eventId: "e1",
          commitSha: null,
          outcome: "failed",
          gate: "just ci",
        }),
      ]),
      rule: {
        state: "not-read",
        reason:
          "This session has no genesis, so there is no mission verdict or land rule to read.",
      },
      main: { state: "not-read", reason: "main was not read." },
      noRepository: true,
    }),
  );
  assert.equal(model.gate.tone, "attention");
  assert.equal(model.gate.unnamedRows.length, 1);
  assert.equal(model.land.word, "no repository");
  assert.equal(model.land.sentence, CODING_SESSION_LANDING_NO_REPOSITORY);
  assert.equal(model.landed.word, "no repository");
  assert.equal(model.landed.sentence, CODING_SESSION_LANDING_NO_REPOSITORY);
  assert.equal(model.landed.checkedAt, null);
  // A rule that answered speaks for itself, even with no repository.
  const answered = codingSessionLandingModel(base({ noRepository: true }));
  assert.equal(answered.land.word, "ready");
});
