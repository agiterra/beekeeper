import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionUmbrellaGenerationLabel,
  codingSessionWireWorkspaceStatus,
  codingSessionWorkspaceStatusDetail,
  deriveCodingSessionExecutionStatus,
  deriveCodingSessionWorkspaceStatus,
  resolveCodingSessionWorkspace,
  umbrellaHasCollapsedHistory,
} from "./codingSessionWorkspaceModel.ts";

const session = {
  generationId: "generation-1",
  label: "Generation 1",
  title: "Coding session",
  lastEventAt: "2026-07-30T12:00:00.000Z",
  status: "unknown",
  transcript: [],
  conflictCount: 0,
  commandTarget: {
    driver: "driver",
    instanceId: "instance",
    sessionId: "session",
    generation: 1,
  },
};

function catalog(overrides = {}) {
  return {
    channelId: "channel-1",
    entries: [],
    isLoading: false,
    errorMessage: null,
    authorityErrorMessage: null,
    rejectedAuthorCount: 0,
    invalidSignatureCount: 0,
    ...overrides,
  };
}

test("workspace resolves only the exact catalog generation", () => {
  const resolution = resolveCodingSessionWorkspace({
    catalog: catalog({ entries: [session] }),
    generationId: "generation-1",
  });
  assert.equal(resolution.kind, "ready");
  assert.equal(resolution.session, session);
  assert.equal(
    resolveCodingSessionWorkspace({
      catalog: catalog({ entries: [session] }),
      generationId: "generation-2",
    }).kind,
    "missing",
  );
});

test("N=1 identity: a ready single-execution resolution is an umbrella of one over the exact record", () => {
  const resolution = resolveCodingSessionWorkspace({
    catalog: catalog({ entries: [session] }),
    generationId: "generation-1",
  });
  assert.equal(resolution.kind, "ready");
  // The routed record is handed to the surface untouched — the single-session
  // tree renders exactly the same catalog record it rendered before Step 4.
  assert.equal(resolution.session, session);
  assert.equal(resolution.umbrella.executions.length, 1);
  assert.equal(resolution.umbrella.sessionRef, null);
  assert.match(resolution.umbrella.umbrellaKey, /^implicit:/);
  assert.equal(resolution.focusedExecution.activeGeneration, session);
});

const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

function umbrellaMember({
  generationId,
  signerPubkey,
  sessionId,
  generation = 1,
  lastEventAt = "2026-07-30T12:00:00.000Z",
}) {
  return {
    ...session,
    generationId,
    providerAuthorityPubkey: signerPubkey,
    sessionRef: SESSION_REF,
    lastEventAt,
    commandTarget: { ...session.commandTarget, sessionId, generation },
  };
}

test("a shared sessionRef resolves one umbrella with the routed execution focused", () => {
  const claude = umbrellaMember({
    generationId: "generation-claude",
    signerPubkey: "a".repeat(64),
    sessionId: "claude-session",
  });
  const codex = umbrellaMember({
    generationId: "generation-codex",
    signerPubkey: "b".repeat(64),
    sessionId: "codex-session",
  });
  const resolution = resolveCodingSessionWorkspace({
    catalog: catalog({ entries: [claude, codex] }),
    generationId: "generation-codex",
  });
  assert.equal(resolution.kind, "ready");
  assert.equal(resolution.session, codex);
  assert.equal(resolution.umbrella.sessionRef, SESSION_REF);
  assert.equal(resolution.umbrella.executions.length, 2);
  assert.equal(resolution.focusedExecution.activeGeneration, codex);
});

test("routing to a collapsed prior generation still resolves the surrounding umbrella", () => {
  const priorGeneration = umbrellaMember({
    generationId: "generation-claude-1",
    signerPubkey: "a".repeat(64),
    sessionId: "claude-session",
    generation: 1,
    lastEventAt: "2026-07-30T10:00:00.000Z",
  });
  const activeGeneration = umbrellaMember({
    generationId: "generation-claude-2",
    signerPubkey: "a".repeat(64),
    sessionId: "claude-session",
    generation: 2,
  });
  const codex = umbrellaMember({
    generationId: "generation-codex",
    signerPubkey: "b".repeat(64),
    sessionId: "codex-session",
  });
  const resolution = resolveCodingSessionWorkspace({
    catalog: catalog({ entries: [priorGeneration, activeGeneration, codex] }),
    generationId: "generation-claude-1",
  });
  assert.equal(resolution.kind, "ready");
  assert.equal(resolution.session, priorGeneration);
  assert.equal(resolution.umbrella.executions.length, 2);
  assert.equal(resolution.focusedExecution.activeGeneration, activeGeneration);
  assert.deepEqual(
    resolution.focusedExecution.priorGenerations.map(
      (record) => record.generationId,
    ),
    ["generation-claude-1"],
  );
});

test("the catalog's create observations resolve founder, operator, and foreign attachments", () => {
  const founder = "f".repeat(64);
  const teammate = "e".repeat(64);
  const claude = umbrellaMember({
    generationId: "generation-claude",
    signerPubkey: "a".repeat(64),
    sessionId: "claude-session",
  });
  const codex = umbrellaMember({
    generationId: "generation-codex",
    signerPubkey: "b".repeat(64),
    sessionId: "codex-session",
  });
  const creates = [
    {
      sessionRef: SESSION_REF,
      signerPubkey: founder,
      createdAt: 1_800_000_000,
      eventId: "event-a",
      target: claude.commandTarget,
    },
    {
      sessionRef: SESSION_REF,
      signerPubkey: teammate,
      createdAt: 1_800_000_100,
      eventId: "event-b",
      target: codex.commandTarget,
    },
  ];

  const resolution = resolveCodingSessionWorkspace({
    catalog: catalog({ entries: [claude, codex], creates }),
    generationId: "generation-codex",
  });
  assert.equal(resolution.kind, "ready");
  assert.equal(resolution.umbrella.founderPubkey, founder);
  // The teammate's execution is flagged, not merged and not hidden.
  assert.equal(resolution.umbrella.foreignAttachmentCount, 1);
  assert.equal(resolution.focusedExecution.operatorPubkey, teammate);

  // Same catalog with the observations still in flight (relay gap, cold
  // start): authority is unknown, so nothing is gated and nothing is accused.
  const unobserved = resolveCodingSessionWorkspace({
    catalog: catalog({ entries: [claude, codex] }),
    generationId: "generation-codex",
  });
  assert.equal(unobserved.umbrella.founderPubkey, null);
  assert.equal(unobserved.umbrella.foreignAttachmentCount, 0);
  assert.equal(unobserved.focusedExecution.operatorPubkey, null);
});

test("missing exact generation distinguishes loading and invalid authority", () => {
  assert.equal(
    resolveCodingSessionWorkspace({
      catalog: catalog({ isLoading: true }),
      generationId: "generation-1",
    }).kind,
    "loading",
  );
  assert.equal(
    resolveCodingSessionWorkspace({
      catalog: catalog({
        authorityErrorMessage: "No trusted bridge configured.",
      }),
      generationId: "generation-1",
    }).kind,
    "untrusted",
  );
});

test("unrelated rejected evidence never changes a missing generation to untrusted", () => {
  assert.equal(
    resolveCodingSessionWorkspace({
      catalog: catalog({
        rejectedAuthorCount: 2,
        invalidSignatureCount: 1,
      }),
      generationId: "generation-missing",
    }).kind,
    "missing",
  );
});

test("workspace status derives from provider-neutral transcript lifecycle", () => {
  assert.deepEqual(
    deriveCodingSessionWorkspaceStatus([
      {
        id: "status-1",
        type: "lifecycle",
        renderClass: "status",
        title: "Status",
        text: "thinking",
        timestamp: "2026-07-30T12:00:00.000Z",
      },
    ]),
    { kind: "working", label: "Working" },
  );
  assert.deepEqual(
    deriveCodingSessionWorkspaceStatus([
      {
        id: "result-1",
        type: "lifecycle",
        renderClass: "status",
        title: "Turn result",
        text: "Done",
        timestamp: "2026-07-30T12:00:00.000Z",
      },
    ]),
    { kind: "idle", label: "Idle" },
  );
  assert.deepEqual(deriveCodingSessionWorkspaceStatus([]), {
    kind: "unknown",
    label: "Status unknown",
  });
  assert.deepEqual(deriveCodingSessionWorkspaceStatus([], "stopped"), {
    kind: "ended",
    label: "Ended",
  });
});

test("empty transcript falls back to the provider's wire status", () => {
  // The user-visible bug: a brand-new session already has signed 44223
  // metadata saying `idle`, but the header showed "Status unknown" until the
  // first turn produced a transcript item.
  assert.deepEqual(deriveCodingSessionWorkspaceStatus([], "idle"), {
    kind: "idle",
    label: "Idle",
  });
  assert.deepEqual(deriveCodingSessionWorkspaceStatus([], "running"), {
    kind: "working",
    label: "Working",
  });
  assert.deepEqual(deriveCodingSessionWorkspaceStatus([], "starting"), {
    kind: "working",
    label: "Starting",
  });
  assert.deepEqual(deriveCodingSessionWorkspaceStatus([], "unknown"), {
    kind: "unknown",
    label: "Status unknown",
  });
  // `waiting_for_input` is its own tier, not a shade of idle (SURFACES §2a).
  assert.deepEqual(
    deriveCodingSessionWorkspaceStatus([], "waiting_for_input"),
    { kind: "waiting", label: "Waiting" },
  );
});

test("transcript lifecycle wins over stale wire status", () => {
  // Metadata still claims `running` after the turn's result item landed —
  // the signed transcript is fresher and the header must settle to Idle.
  assert.deepEqual(
    deriveCodingSessionWorkspaceStatus(
      [
        {
          id: "result-1",
          type: "lifecycle",
          renderClass: "status",
          title: "Turn result",
          text: "Done",
          timestamp: "2026-07-30T12:00:00.000Z",
        },
      ],
      "running",
    ),
    { kind: "idle", label: "Idle" },
  );
});

test("a streaming turn after an earlier result reads Working, and settles back", () => {
  const turnOneResult = {
    id: "result-1",
    type: "lifecycle",
    renderClass: "status",
    title: "Turn result",
    text: "Done",
    timestamp: "2026-07-30T12:00:00.000Z",
    turnId: "turn-1",
  };
  // Turn 2 streams only non-lifecycle items — no "Status" markers exist on
  // the normal path, which is exactly why the lifecycle-only scan got stuck.
  const turnTwoStreaming = [
    turnOneResult,
    {
      id: "msg-2",
      type: "message",
      renderClass: "message",
      role: "user",
      title: "Prompt",
      text: "next task",
      timestamp: "2026-07-30T12:05:00.000Z",
      turnId: "turn-2",
    },
  ];
  assert.deepEqual(
    deriveCodingSessionWorkspaceStatus(turnTwoStreaming, "running"),
    { kind: "working", label: "Working" },
  );
  // Turn 2's terminator closes it again.
  assert.deepEqual(
    deriveCodingSessionWorkspaceStatus(
      [
        ...turnTwoStreaming,
        { ...turnOneResult, id: "result-2", turnId: "turn-2" },
      ],
      "idle",
    ),
    { kind: "idle", label: "Idle" },
  );
});

test("an initial turn with no terminator yet reads Working", () => {
  assert.deepEqual(
    deriveCodingSessionWorkspaceStatus(
      [
        {
          id: "msg-1",
          type: "message",
          renderClass: "message",
          role: "user",
          title: "Prompt",
          text: "go",
          timestamp: "2026-07-30T12:00:00.000Z",
          turnId: "turn-1",
        },
      ],
      "running",
    ),
    { kind: "working", label: "Working" },
  );
});

test("metadata newer than the whole transcript speaks for the session", () => {
  const transcript = [
    {
      id: "result-1",
      type: "lifecycle",
      renderClass: "status",
      title: "Turn result",
      text: "Done",
      timestamp: "2026-07-30T12:00:00.000Z",
      turnId: "turn-1",
    },
  ];
  const afterTranscript = Date.parse("2026-07-30T12:01:00.000Z");
  // TurnStarted publishes `running` before any turn-2 transcript item exists.
  assert.deepEqual(
    deriveCodingSessionWorkspaceStatus(transcript, "running", afterTranscript),
    { kind: "working", label: "Working" },
  );
  // A provider that died mid-turn reports it the same way — and the header
  // must say so, never a calm Idle over a Reconnect banner.
  assert.deepEqual(
    deriveCodingSessionWorkspaceStatus(
      transcript,
      "disconnected",
      afterTranscript,
    ),
    { kind: "unknown", label: "Disconnected", attention: "disconnected" },
  );
  // Stale metadata (older than the transcript) never overrides the scan.
  const beforeTranscript = Date.parse("2026-07-30T11:00:00.000Z");
  assert.deepEqual(
    deriveCodingSessionWorkspaceStatus(transcript, "running", beforeTranscript),
    { kind: "idle", label: "Idle" },
  );
});

test("a signed lifecycle status outranks anything the transcript implies", () => {
  // The provider's crash recovery synthesizes a terminal "Turn result" item,
  // so transcript inference alone reads a disconnected execution as Idle —
  // the header saying "Idle" over a composer offering Reconnect.
  const recoveredTranscript = [
    {
      id: "result-1",
      type: "lifecycle",
      renderClass: "status",
      title: "Turn result",
      text: "Done",
      timestamp: "2026-07-30T12:00:00.000Z",
    },
  ];
  assert.deepEqual(
    deriveCodingSessionWorkspaceStatus(recoveredTranscript, "disconnected"),
    { kind: "unknown", label: "Disconnected", attention: "disconnected" },
  );
  assert.deepEqual(
    deriveCodingSessionWorkspaceStatus(recoveredTranscript, "failed"),
    { kind: "unknown", label: "Needs attention", attention: "failed" },
  );
  // A durable stop still ends, and an unknown lifecycle still falls back to
  // reading the transcript.
  assert.deepEqual(
    deriveCodingSessionWorkspaceStatus(recoveredTranscript, "stopped"),
    { kind: "ended", label: "Ended" },
  );
  assert.deepEqual(
    deriveCodingSessionWorkspaceStatus(recoveredTranscript, undefined),
    { kind: "idle", label: "Idle" },
  );
  assert.deepEqual(
    deriveCodingSessionWorkspaceStatus(
      [
        {
          id: "status-1",
          type: "lifecycle",
          renderClass: "status",
          title: "Status",
          text: "running",
          timestamp: "2026-07-30T12:00:00.000Z",
        },
      ],
      "running",
    ),
    { kind: "working", label: "Working" },
  );
});

test("a routed disconnected generation self-heals onto the generation its resume minted", () => {
  // A resume mints the NEXT generation; the routed one keeps its immutable
  // `disconnected` metadata forever. Without this, a missed receipt (or an app
  // restart mid-resume) leaves the workspace pinned to a dead generation whose
  // banner can never clear.
  const dead = {
    ...umbrellaMember({
      generationId: "generation-claude-1",
      signerPubkey: "a".repeat(64),
      sessionId: "claude-session",
      generation: 1,
      lastEventAt: "2026-07-30T10:00:00.000Z",
    }),
    status: "disconnected",
  };
  const resumed = {
    ...umbrellaMember({
      generationId: "generation-claude-2",
      signerPubkey: "a".repeat(64),
      sessionId: "claude-session",
      generation: 2,
    }),
    status: "idle",
  };
  const resolution = resolveCodingSessionWorkspace({
    catalog: catalog({ entries: [dead, resumed] }),
    generationId: "generation-claude-1",
  });
  assert.equal(resolution.kind, "ready");
  assert.equal(resolution.session, resumed);
  assert.equal(resolution.focusedExecution.activeGeneration, resumed);

  // Routing to the active generation is untouched, and a prior generation that
  // was not disconnected still opens as itself — history deep links are a
  // person reading the past, not a stale route.
  assert.equal(
    resolveCodingSessionWorkspace({
      catalog: catalog({ entries: [dead, resumed] }),
      generationId: "generation-claude-2",
    }).session,
    resumed,
  );
  assert.equal(
    resolveCodingSessionWorkspace({
      catalog: catalog({
        entries: [{ ...dead, status: "stopped" }, resumed],
      }),
      generationId: "generation-claude-1",
    }).session.generationId,
    "generation-claude-1",
  );
});

test("a session-scoped continuity fact is not mistaken for a streaming turn", () => {
  // The live regression: `session_fresh` is the first (and only) transcript
  // item a brand-new session has. It belongs to no turn, so the open-turn
  // heuristic must not read it as one — the header said WORKING with an
  // Interrupt button over an idle provider that had never run a turn.
  const continuityRow = {
    id: "status-1",
    type: "lifecycle",
    renderClass: "status",
    title: "Session continuity",
    text: "Started fresh — no prior session context",
    timestamp: "2026-08-18T10:00:01.000Z",
  };
  assert.deepEqual(
    deriveCodingSessionWorkspaceStatus([continuityRow], "idle"),
    {
      kind: "idle",
      label: "Idle",
    },
  );
  // …and the same fact must not mask a turn that really is streaming.
  assert.deepEqual(
    deriveCodingSessionWorkspaceStatus(
      [
        continuityRow,
        {
          id: "msg-1",
          type: "message",
          renderClass: "message",
          role: "user",
          title: "Prompt",
          text: "go",
          timestamp: "2026-08-18T10:00:05.000Z",
          turnId: "turn-1",
        },
      ],
      "running",
    ),
    { kind: "working", label: "Working" },
  );
});

function umbrellaOf(executions) {
  return { executions };
}

function executionOf({ generation = 1, priorGenerations = [] } = {}) {
  return {
    activeGeneration: {
      ...session,
      commandTarget: { ...session.commandTarget, generation },
    },
    priorGenerations,
  };
}

test("a resumed single execution routes to the umbrella surface", () => {
  // The regression this guards: one execution, two generations. Routing on
  // execution count alone sent this to the flat tree, which renders only the
  // active generation — so a resume read as an erased transcript.
  const umbrella = umbrellaOf([
    executionOf({ generation: 2, priorGenerations: [session] }),
  ]);
  assert.equal(umbrellaHasCollapsedHistory(umbrella), true);
});

test("a single execution with no prior generations stays on the flat tree", () => {
  assert.equal(
    umbrellaHasCollapsedHistory(umbrellaOf([executionOf({ generation: 1 })])),
    false,
  );
});

test("more than one execution still routes to the umbrella surface", () => {
  assert.equal(
    umbrellaHasCollapsedHistory(
      umbrellaOf([executionOf(), executionOf({ generation: 1 })]),
    ),
    true,
  );
});

test("the umbrella label counts executions only when there are several", () => {
  assert.equal(
    codingSessionUmbrellaGenerationLabel(
      umbrellaOf([executionOf(), executionOf()]),
    ),
    "2 executions",
  );
});

test("a resumed single execution is labelled by generation, never '1 executions'", () => {
  const label = codingSessionUmbrellaGenerationLabel(
    umbrellaOf([executionOf({ generation: 2, priorGenerations: [session] })]),
  );
  assert.equal(label, "generation 2 · 1 earlier");
  assert.ok(!label.includes("1 executions"));
});

test("an umbrella with nothing collapsed contributes no label", () => {
  assert.equal(
    codingSessionUmbrellaGenerationLabel(umbrellaOf([executionOf()])),
    "",
  );
});

// §2 item 41 — a status is what a provider said; reachability is whether
// anything can still answer. The header read IDLE for two hours over an app
// that had quit, and the composer offered Send and Stop the whole time.
const IDLE_TRANSCRIPT = [
  {
    type: "lifecycle",
    title: "Turn result",
    text: "ended normally",
    timestamp: "2026-08-23T21:00:00.000Z",
    turnId: "turn-1",
  },
];
const NOW = Date.parse("2026-08-23T23:17:00.000Z");
const REPORTED_AT = Date.parse("2026-08-23T21:17:00.000Z");

test("an unreachable provider demotes Idle to a dated report", () => {
  const status = deriveCodingSessionWorkspaceStatus(
    IDLE_TRANSCRIPT,
    "idle",
    REPORTED_AT,
    { known: true, reachable: false },
    NOW,
  );
  assert.equal(status.kind, "unknown");
  assert.equal(status.label, "No provider answering");
  assert.equal(status.attention, "unreachable");
  assert.deepEqual(status.lastReported, { label: "Idle", ageSeconds: 7_200 });
  assert.equal(
    codingSessionWorkspaceStatusDetail(status),
    "last reported Idle 2h ago",
  );
});

test("a working execution is demoted the same way — a lease, not a mood", () => {
  const status = deriveCodingSessionWorkspaceStatus(
    [
      {
        type: "message",
        title: "Assistant",
        text: "working on it",
        timestamp: "2026-08-23T21:16:00.000Z",
        turnId: "turn-2",
      },
    ],
    "running",
    REPORTED_AT,
    { known: true, reachable: false },
    NOW,
  );
  assert.equal(status.label, "No provider answering");
  assert.equal(status.lastReported.label, "Working");
});

test("a live lease leaves the reported status exactly as it was", () => {
  const status = deriveCodingSessionWorkspaceStatus(
    IDLE_TRANSCRIPT,
    "idle",
    REPORTED_AT,
    { known: true, reachable: true },
    NOW,
  );
  assert.deepEqual(status, { kind: "idle", label: "Idle" });
  assert.equal(codingSessionWorkspaceStatusDetail(status), null);
});

test("an unknown reachability never demotes — absence of evidence is not evidence", () => {
  for (const reachability of [undefined, { known: false }]) {
    const status = deriveCodingSessionWorkspaceStatus(
      IDLE_TRANSCRIPT,
      "idle",
      REPORTED_AT,
      reachability,
      NOW,
    );
    assert.deepEqual(status, { kind: "idle", label: "Idle" });
  }
});

test("signed terminal and attention states outrank reachability", () => {
  const stopped = deriveCodingSessionWorkspaceStatus(
    IDLE_TRANSCRIPT,
    "stopped",
    REPORTED_AT,
    { known: true, reachable: false },
    NOW,
  );
  assert.deepEqual(stopped, { kind: "ended", label: "Ended" });

  const disconnected = deriveCodingSessionWorkspaceStatus(
    [],
    "disconnected",
    REPORTED_AT,
    { known: true, reachable: false },
    NOW,
  );
  assert.equal(disconnected.label, "Disconnected");
  assert.equal(disconnected.attention, "disconnected");
});

test("a demoted status with no observation time says so rather than guessing", () => {
  const status = deriveCodingSessionWorkspaceStatus(
    IDLE_TRANSCRIPT,
    "idle",
    null,
    { known: true, reachable: false },
    NOW,
  );
  assert.deepEqual(status.lastReported, { label: "Idle", ageSeconds: null });
  assert.equal(
    codingSessionWorkspaceStatusDetail(status),
    "last reported Idle",
  );
});

test("a report from seconds ago reads as English, not 'just now ago'", () => {
  const status = deriveCodingSessionWorkspaceStatus(
    IDLE_TRANSCRIPT,
    "idle",
    NOW - 5_000,
    { known: true, reachable: false },
    NOW,
  );
  assert.equal(
    codingSessionWorkspaceStatusDetail(status),
    "last reported Idle just now",
  );
});

// --- W1: one liveness word, from the wire (SURFACES §15(b), §2a) -------------

test("a completed execution with an unterminated transcript is idle, not working", () => {
  // Walk finding 2: the promotion path discarded the signed status before it
  // ever read it, and the strip printed that inference as the word `live`.
  const openTurn = [
    {
      id: "result-1",
      type: "lifecycle",
      renderClass: "status",
      title: "Turn result",
      text: "Done",
      timestamp: "2026-08-12T10:00:00.000Z",
      turnId: "turn-1",
    },
    {
      id: "msg-2",
      type: "message",
      renderClass: "message",
      role: "user",
      title: "Prompt",
      text: "next task",
      timestamp: "2026-08-12T10:05:00.000Z",
      turnId: "turn-2",
    },
  ];
  assert.deepEqual(deriveCodingSessionWorkspaceStatus(openTurn, "completed"), {
    kind: "idle",
    label: "Idle",
  });
  // The same shape a killed seat leaves: a resting `idle`, no terminator.
  assert.deepEqual(deriveCodingSessionWorkspaceStatus(openTurn, "idle"), {
    kind: "idle",
    label: "Idle",
  });
});

test("a live seat with an open turn still narrows to working", () => {
  const openTurn = [
    {
      id: "msg-1",
      type: "message",
      renderClass: "message",
      role: "user",
      title: "Prompt",
      text: "go",
      timestamp: "2026-08-12T10:05:00.000Z",
      turnId: "turn-2",
    },
  ];
  assert.deepEqual(deriveCodingSessionWorkspaceStatus(openTurn, "running"), {
    kind: "working",
    label: "Working",
  });
});

test("a waiting_for_input seat reads the waiting word, not idle", () => {
  assert.deepEqual(
    deriveCodingSessionWorkspaceStatus([], "waiting_for_input"),
    { kind: "waiting", label: "Waiting" },
  );
  assert.deepEqual(codingSessionWireWorkspaceStatus("waiting_for_input"), {
    kind: "waiting",
    label: "Waiting",
  });
});

test("the transcript heuristic never produces the waiting word — and never hides one", () => {
  const openTurn = [
    {
      id: "msg-1",
      type: "message",
      renderClass: "message",
      role: "user",
      title: "Prompt",
      text: "go",
      timestamp: "2026-08-12T10:05:00.000Z",
      turnId: "turn-2",
    },
  ];
  // An open turn on a waiting seat does not invent activity.
  assert.deepEqual(
    deriveCodingSessionWorkspaceStatus(openTurn, "waiting_for_input"),
    { kind: "waiting", label: "Waiting" },
  );
  // And no transcript row can mint the waiting kind on its own.
  assert.deepEqual(
    deriveCodingSessionWorkspaceStatus(
      [
        {
          id: "status-1",
          type: "lifecycle",
          renderClass: "status",
          title: "Status",
          text: "waiting_for_input",
          timestamp: "2026-08-12T10:05:00.000Z",
        },
      ],
      "running",
    ),
    { kind: "working", label: "Working" },
  );
});

test("an unreachable waiting seat is demoted exactly as a working one is", () => {
  const status = deriveCodingSessionWorkspaceStatus(
    [],
    "waiting_for_input",
    Date.parse("2026-08-12T10:00:00.000Z"),
    { known: true, reachable: false },
    Date.parse("2026-08-12T10:04:00.000Z"),
  );
  assert.deepEqual(status, {
    kind: "unknown",
    label: "No provider answering",
    attention: "unreachable",
    lastReported: { label: "Waiting", ageSeconds: 240 },
  });
});

test("one execution, one status: the rail and the strip read the same function", () => {
  const openTurn = [
    {
      id: "msg-1",
      type: "message",
      renderClass: "message",
      role: "user",
      title: "Prompt",
      text: "go",
      timestamp: "2026-08-12T10:05:00.000Z",
      turnId: "turn-2",
    },
  ];
  const built = {
    executionKey: "execution-poker",
    signerPubkey: "a".repeat(64),
    operatorPubkey: null,
    priorGenerations: [],
    activeGeneration: {
      ...session,
      status: "running",
      statusAt: Date.parse("2026-08-12T10:00:00.000Z"),
      transcript: openTurn,
    },
  };
  assert.deepEqual(
    deriveCodingSessionExecutionStatus(
      built,
      { known: true, reachable: false },
      Date.parse("2026-08-12T10:04:00.000Z"),
    ),
    {
      kind: "unknown",
      label: "No provider answering",
      attention: "unreachable",
      lastReported: { label: "Working", ageSeconds: 240 },
    },
  );
  assert.deepEqual(
    deriveCodingSessionExecutionStatus(built, { known: false }),
    { kind: "working", label: "Working" },
  );
});
