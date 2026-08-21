import assert from "node:assert/strict";
import test from "node:test";

import {
  listCodingSessionMentionTargets,
  parseLeadingCodingSessionMention,
  resolveCodingSessionMention,
  stripCodingSessionMentionForTarget,
  suggestCodingSessionMentionHandles,
} from "./codingSessionMentionRouting.ts";
import {
  groupCodingSessionCatalog,
  listCodingSessionUmbrellaParticipants,
} from "./codingSessionUmbrellaModel.ts";

const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

function record({
  signerPubkey,
  driver,
  runtime,
  provider = null,
  model = null,
  status = "idle",
  lastEventAt = "2026-08-12T10:00:00.000Z",
  commandTarget = undefined,
  capabilities = null,
}) {
  return {
    generationId: `gen-${signerPubkey.slice(0, 6)}-${driver}`,
    label: `${driver} · generation 1`,
    title: "Advance Buzz live sessions",
    providerAuthorityPubkey: signerPubkey,
    metadataAuthorityPubkey: signerPubkey,
    lastEventAt,
    status,
    transcript: [],
    conflictCount: 0,
    commandTarget:
      commandTarget === undefined
        ? {
            driver,
            instanceId: `${driver}-instance-${signerPubkey.slice(0, 4)}`,
            sessionId: `${signerPubkey.slice(0, 8)}-1111-1111-1111-111111111111`,
            generation: 1,
          }
        : commandTarget,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    provider,
    runtime,
    model,
    capabilities,
  };
}

function participantsFor(records) {
  const [umbrella] = groupCodingSessionCatalog(records);
  return listCodingSessionUmbrellaParticipants(umbrella);
}

const CLAUDE = record({
  signerPubkey: "a".repeat(64),
  driver: "claude-agent-acp",
  runtime: "claude",
  provider: "claude-primary",
  model: "claude-opus-5",
  lastEventAt: "2026-08-12T11:00:00.000Z",
});
const CODEX = record({
  signerPubkey: "b".repeat(64),
  driver: "codex-acp",
  runtime: "codex",
  provider: "codex-primary",
  model: "gpt-5.3-codex",
});

function pair() {
  return participantsFor([CLAUDE, CODEX]);
}

function keyOf(participants, runtime) {
  const participant = participants.find(
    (entry) =>
      entry.kind === "execution" &&
      entry.execution.activeGeneration.runtime === runtime,
  );
  return `execution:${participant.executionKey}`;
}

// --- parsing ---------------------------------------------------------------

test("a leading handle parses; the delimiter and surrounding space go with it", () => {
  assert.deepEqual(parseLeadingCodingSessionMention("@codex rerun the build"), {
    handle: "codex",
    raw: "codex",
    rest: "rerun the build",
  });
  assert.deepEqual(
    parseLeadingCodingSessionMention("@codex, rerun the build"),
    { handle: "codex", raw: "codex", rest: "rerun the build" },
  );
  assert.deepEqual(parseLeadingCodingSessionMention("@codex: rerun"), {
    handle: "codex",
    raw: "codex",
    rest: "rerun",
  });
  // A bare handle is still a handle; the prompt is simply empty so far.
  assert.deepEqual(parseLeadingCodingSessionMention("@codex"), {
    handle: "codex",
    raw: "codex",
    rest: "",
  });
  // Newlines and spaces before the handle are still "leading".
  assert.deepEqual(parseLeadingCodingSessionMention("\n  @codex fix"), {
    handle: "codex",
    raw: "codex",
    rest: "fix",
  });
});

test("only a leading handle parses — mid-sentence mentions stay prose", () => {
  assert.equal(
    parseLeadingCodingSessionMention("ask @codex about the fixture"),
    null,
  );
  assert.equal(parseLeadingCodingSessionMention("see mail@codex.dev"), null);
  assert.equal(parseLeadingCodingSessionMention("no handle here"), null);
  assert.equal(parseLeadingCodingSessionMention(""), null);
});

test("a handle butted against a non-delimiter is a literal token", () => {
  assert.equal(
    parseLeadingCodingSessionMention("@codex/foo.ts is broken"),
    null,
  );
  assert.equal(parseLeadingCodingSessionMention("@codex's turn"), null);
  assert.equal(parseLeadingCodingSessionMention("@codex,fix"), null);
});

test("handles normalize case and separators", () => {
  assert.equal(parseLeadingCodingSessionMention("@Claude go").handle, "claude");
  assert.equal(parseLeadingCodingSessionMention("@CODEX go").handle, "codex");
  assert.equal(
    parseLeadingCodingSessionMention("@Claude_Primary go").handle,
    "claude-primary",
  );
  // The raw text is preserved for echoing back what was typed.
  assert.equal(parseLeadingCodingSessionMention("@Claude go").raw, "Claude");
});

// --- matching --------------------------------------------------------------

test("a leading handle routes to that execution and leaves the prompt clean", () => {
  const participants = pair();
  const resolution = resolveCodingSessionMention({
    participants,
    text: "@codex rerun the failing fixture",
  });
  assert.equal(resolution.kind, "match");
  assert.equal(resolution.participantKey, keyOf(participants, "codex"));
  assert.equal(resolution.text, "rerun the failing fixture");
  assert.match(resolution.label, /Codex/);
});

test("runtime, provider ref, and their leading token all address the execution", () => {
  const participants = pair();
  const codexKey = keyOf(participants, "codex");
  for (const text of ["@codex go", "@codex-primary go", "@CODEX-Primary go"]) {
    const resolution = resolveCodingSessionMention({ participants, text });
    assert.equal(resolution.kind, "match", text);
    assert.equal(resolution.participantKey, codexKey, text);
    assert.equal(resolution.text, "go", text);
  }
});

test("the driver is a handle only when no runtime or provider was published", () => {
  const participants = participantsFor([
    record({
      signerPubkey: "c".repeat(64),
      driver: "gemini-acp",
      runtime: null,
      provider: null,
    }),
    CODEX,
  ]);
  const resolution = resolveCodingSessionMention({
    participants,
    text: "@gemini-acp take a look",
  });
  assert.equal(resolution.kind, "match");
  assert.equal(resolution.text, "take a look");
  assert.equal(
    resolveCodingSessionMention({ participants, text: "@gemini look" }).kind,
    "match",
  );
  // Claude's runtime/provider are published, so its driver token is not a handle.
  assert.equal(
    resolveCodingSessionMention({
      participants: pair(),
      text: "@claude-agent-acp look",
    }).kind,
    "unknown",
  );
});

test("a mid-sentence handle never retargets", () => {
  const participants = pair();
  const resolution = resolveCodingSessionMention({
    participants,
    text: "ask @codex to rerun the fixture",
  });
  assert.deepEqual(resolution, { kind: "none" });
});

test("an unknown handle is ordinary text, never a silent misroute", () => {
  const resolution = resolveCodingSessionMention({
    participants: pair(),
    text: "@gemini please look",
  });
  assert.equal(resolution.kind, "unknown");
  assert.equal(resolution.handle, "gemini");
});

// --- ambiguity -------------------------------------------------------------

test("two executions answering to one handle is reported, never guessed", () => {
  const participants = participantsFor([
    CLAUDE,
    record({
      signerPubkey: "d".repeat(64),
      driver: "claude-agent-acp",
      runtime: "claude",
      provider: "claude-secondary",
      model: "claude-sonnet-5",
    }),
  ]);
  const ambiguous = resolveCodingSessionMention({
    participants,
    text: "@claude rerun",
  });
  assert.equal(ambiguous.kind, "ambiguous");
  assert.equal(ambiguous.labels.length, 2);
  // The prompt is untouched: the explicit selector decides.
  assert.equal(
    stripCodingSessionMentionForTarget({
      participants,
      participantKey: keyOf(participants, "claude"),
      text: "@claude rerun",
    }),
    "@claude rerun",
  );
  // The provider instance refs stay unambiguous, so they still address one.
  const specific = resolveCodingSessionMention({
    participants,
    text: "@claude-secondary rerun",
  });
  assert.equal(specific.kind, "match");
  assert.equal(specific.text, "rerun");
  assert.equal(
    listCodingSessionMentionTargets(participants).some(
      (target) => target.handle === "claude",
    ),
    false,
  );
});

// --- availability ----------------------------------------------------------

test("a failed or ungoverned execution is never routed to silently", () => {
  const failed = participantsFor([
    CODEX,
    record({
      signerPubkey: "e".repeat(64),
      driver: "claude-agent-acp",
      runtime: "claude",
      status: "failed",
    }),
  ]);
  const resolution = resolveCodingSessionMention({
    participants: failed,
    text: "@claude retry",
  });
  assert.equal(resolution.kind, "unavailable");
  assert.match(resolution.reason, /failed/);
  assert.equal(
    stripCodingSessionMentionForTarget({
      participants: failed,
      participantKey: keyOf(failed, "claude"),
      text: "@claude retry",
    }),
    "@claude retry",
  );

  const untargeted = participantsFor([
    CODEX,
    record({
      signerPubkey: "f".repeat(64),
      driver: "claude-agent-acp",
      runtime: "claude",
      commandTarget: null,
    }),
  ]);
  const pending = resolveCodingSessionMention({
    participants: untargeted,
    text: "@claude retry",
  });
  assert.equal(pending.kind, "unavailable");
  assert.match(pending.reason, /command target/);

  const noTurns = participantsFor([
    CODEX,
    record({
      signerPubkey: "9".repeat(64),
      driver: "claude-agent-acp",
      runtime: "claude",
      capabilities: {
        threadTurnStart: false,
        threadTurnInterrupt: false,
        threadSteer: false,
        context: false,
        diff: false,
        plan: false,
      },
    }),
  ]);
  assert.equal(
    resolveCodingSessionMention({ participants: noTurns, text: "@claude go" })
      .kind,
    "unavailable",
  );
});

// --- stripping -------------------------------------------------------------

test("the handle is stripped only for the participant it actually routed to", () => {
  const participants = pair();
  assert.equal(
    stripCodingSessionMentionForTarget({
      participants,
      participantKey: keyOf(participants, "codex"),
      text: "@codex rerun the fixture",
    }),
    "rerun the fixture",
  );
  // Same text, sent to Claude (explicit selector): the words stay as typed.
  assert.equal(
    stripCodingSessionMentionForTarget({
      participants,
      participantKey: keyOf(participants, "claude"),
      text: "@codex rerun the fixture",
    }),
    "@codex rerun the fixture",
  );
  assert.equal(
    stripCodingSessionMentionForTarget({
      participants,
      participantKey: null,
      text: "@codex rerun the fixture",
    }),
    "@codex rerun the fixture",
  );
  assert.equal(
    stripCodingSessionMentionForTarget({
      participants,
      participantKey: keyOf(participants, "codex"),
      text: "please ask @codex to rerun",
    }),
    "please ask @codex to rerun",
  );
});

// --- discoverability -------------------------------------------------------

test("one suggested handle per addressable execution, shortest first, in selector order", () => {
  const participants = pair();
  // Selector order is attach order, which for these fixtures puts Codex first.
  assert.deepEqual(
    participants
      .filter((participant) => participant.kind === "execution")
      .map((participant) => participant.execution.activeGeneration.runtime),
    ["codex", "claude"],
  );
  assert.deepEqual(
    suggestCodingSessionMentionHandles(participants).map(
      (target) => target.handle,
    ),
    ["codex", "claude"],
  );
  const withFailure = participantsFor([
    CODEX,
    record({
      signerPubkey: "e".repeat(64),
      driver: "claude-agent-acp",
      runtime: "claude",
      status: "failed",
    }),
  ]);
  assert.deepEqual(
    suggestCodingSessionMentionHandles(withFailure).map(
      (target) => target.handle,
    ),
    ["codex"],
  );
});

// --- the N=1 guarantee -----------------------------------------------------

test("a single-execution session has no handles at all", () => {
  const participants = participantsFor([CLAUDE]);
  assert.equal(participants.length, 1);
  assert.deepEqual(listCodingSessionMentionTargets(participants), []);
  assert.deepEqual(suggestCodingSessionMentionHandles(participants), []);
  assert.deepEqual(
    resolveCodingSessionMention({ participants, text: "@claude rerun" }),
    { kind: "none" },
  );
  assert.equal(
    stripCodingSessionMentionForTarget({
      participants,
      participantKey: `execution:${participants[0].executionKey}`,
      text: "@claude rerun",
    }),
    "@claude rerun",
  );
});
