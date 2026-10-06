import {
  CODING_SESSION_OBSERVATION_FOLD_COMMAND,
  CODING_SESSION_OBSERVATION_FOLD_RESPONSE_SCHEMA,
} from "@/features/coding-sessions/lib/codingSessionObservationWire";
import {
  CODING_SESSION_TEAM_FOLD_COMMAND,
  CODING_SESSION_TEAM_FOLD_RESPONSE_SCHEMA,
} from "@/features/coding-sessions/lib/invokeCodingSessionTeamFold";
import type {
  WaveBMockCommandConfig,
  WaveBMockCommandResult,
} from "./e2eBridgeWaveBRegistry";

/**
 * Mock Tauri commands for surface badges (SV-22, SV-41), owned by lane B3.
 *
 * Off unless a spec sets `window.__BEEKEEPER_E2E_WAVE_B_BADGES_FOLDS__ = true`
 * before the app loads. Then the two native folds a badge reads through —
 * the kind-44246 observation fold and the kind-44244 team fold — answer
 * from the events they are actually handed, so a spec can seed signed
 * events live and watch a badge appear and clear without swapping canned
 * responses between steps.
 *
 * These are deliberately small folds, not `buzz-core`'s: they echo every
 * input, include every transaction, and pair a gate start with its close by
 * author, gate and start time. The real folds' semantics are tested in Rust;
 * this exists so the screen can be driven by live signed events.
 */
type MockEvent = {
  id: string;
  pubkey: string;
  created_at: number;
  kind: number;
  tags: string[][];
  content: string;
};

function enabled(): boolean {
  return (
    typeof window !== "undefined" &&
    (window as Window & { __BEEKEEPER_E2E_WAVE_B_BADGES_FOLDS__?: boolean })
      .__BEEKEEPER_E2E_WAVE_B_BADGES_FOLDS__ === true
  );
}

function parse(event: MockEvent): Record<string, unknown> | null {
  try {
    const value = JSON.parse(event.content) as unknown;
    return typeof value === "object" && value !== null
      ? (value as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

function record(value: unknown): Record<string, unknown> {
  return typeof value === "object" && value !== null
    ? (value as Record<string, unknown>)
    : {};
}

const OBSERVATION_DISCLOSURE =
  "an observation is something its author saw, not a decision: it settles nothing, authorizes nothing and excludes nothing, and every duration in it is the author's own measurement";

/** The observation fold, from the events it was handed. */
function foldObservations(request: Record<string, unknown>) {
  const events = (request.events as MockEvent[] | undefined) ?? [];
  const gates: Record<string, unknown>[] = [];
  const starts: Record<string, unknown>[] = [];
  const closes: { event: MockEvent; body: Record<string, unknown> }[] = [];
  // Relay pages arrive newest first; fold oldest first, as the adapter does.
  for (const event of [...events].reverse()) {
    const content = parse(event);
    if (!content) continue;
    const body = record(content.body);
    const assignmentRef =
      typeof content.assignmentRef === "string" ? content.assignmentRef : null;
    if (content.type === "gate") {
      for (const row of (body.rows as Record<string, unknown>[]) ?? []) {
        gates.push({
          authorPubkey: event.pubkey,
          source: content.source,
          eventIds: [event.id],
          droppedEventIds: 0,
          assignmentRef,
          gate: row.gate,
          outcome: row.outcome,
          command: row.command,
          summary: row.summary ?? null,
          durationMs: row.durationMs ?? null,
          headSha: row.headSha ?? null,
          dirty: row.dirty ?? null,
        });
      }
    }
    if (
      content.type === "phase" &&
      typeof body.phase === "string" &&
      body.phase.startsWith("gate:")
    ) {
      if (body.endedAtMs === null) {
        starts.push({
          eventId: event.id,
          authorPubkey: event.pubkey,
          gate: body.phase.slice("gate:".length),
          startedAtMs: body.startedAtMs,
          assignmentRef,
          closeEventId: null,
          endedAtMs: null,
          durationMs: null,
        });
      } else {
        closes.push({ event, body });
      }
    }
  }
  for (const { event, body } of closes) {
    const start = starts.find(
      (candidate) =>
        candidate.authorPubkey === event.pubkey &&
        `gate:${candidate.gate}` === body.phase &&
        candidate.startedAtMs === body.startedAtMs &&
        candidate.closeEventId === null,
    );
    if (!start) continue;
    start.closeEventId = event.id;
    start.endedAtMs = body.endedAtMs;
    start.durationMs = body.durationMs ?? null;
  }
  return {
    schema: CODING_SESSION_OBSERVATION_FOLD_RESPONSE_SCHEMA,
    implementation: "buzz-core",
    inputEventIds: events.map((event) => event.id),
    sessionRef: request.sessionRef,
    genesisRef: request.genesisRef,
    checkpoints: [],
    gates,
    findings: [],
    phases: [],
    gateStarts: starts,
    gateStartStaleAfterMs: 1_800_000,
    unresolved: [],
    ignored: [],
    misclaimedObserved: [],
    provenanceChecked: true,
    truncated: {
      checkpoints: 0,
      gates: 0,
      findings: 0,
      phases: 0,
      unresolved: 0,
      ignored: 0,
      misclaimedObserved: 0,
      entryEventIds: 0,
      displacedGates: 0,
      displacedFindings: 0,
      gateStarts: 0,
      gateStartClosesUnmatched: 0,
    },
    disclosure: OBSERVATION_DISCLOSURE,
  };
}

/** The team fold, from the transactions it was handed. */
function foldTeam(request: Record<string, unknown>) {
  const context = record(request.context);
  const events = (request.events as MockEvent[] | undefined) ?? [];
  const inputEventIds = (request.inputEventIds as string[] | undefined) ?? [];
  const typed = events.map((event) => {
    const content = parse(event) ?? {};
    return {
      event,
      type: content.type,
      body: record(content.body),
    };
  });
  const assignments = typed
    .filter((entry) => entry.type === "assignment")
    .map((assignment) => {
      const report = typed.find(
        (entry) =>
          entry.type === "report" &&
          entry.body.assignmentRef === assignment.event.id,
      );
      const assignee =
        typeof assignment.body.assigneeActor === "string"
          ? assignment.body.assigneeActor
          : null;
      return {
        assignmentEventId: assignment.event.id,
        governedReportEventId: report?.event.id ?? null,
        dispositionEventId: null,
        acknowledgementEventId: null,
        settled: false,
        settledBy: null,
        awaiting: report
          ? { link: "disposition", owedByRole: "lead", owedByActor: null }
          : {
              link: "report",
              owedByRole:
                typeof assignment.body.assigneeRole === "string"
                  ? assignment.body.assigneeRole
                  : "builder",
              owedByActor: assignee,
            },
      };
    });
  const decisions = typed
    .filter((entry) => entry.type === "decision.request")
    .map((request) => {
      const answer = typed.find(
        (entry) =>
          entry.type === "decision.answer" &&
          entry.body.requestRef === request.event.id,
      );
      return {
        requestId: request.event.id,
        heldOn:
          typeof request.body.heldOn === "string"
            ? request.body.heldOn
            : "founder",
        blocks: Array.isArray(request.body.blocks) ? request.body.blocks : [],
        answeredBy: answer ? answer.event.pubkey : null,
        answerId: answer ? answer.event.id : null,
      };
    });
  const waiting = decisions.find((decision) => decision.answerId === null);
  return {
    schema: CODING_SESSION_TEAM_FOLD_RESPONSE_SCHEMA,
    implementation: "buzz-core",
    inputEventIds,
    context: {
      channelRef: context.channelRef,
      sessionRef: context.sessionRef,
      genesisRef: context.genesisRef,
      founderPubkey: context.founderPubkey,
      authorityHeadEventId: context.authorityHeadEventId ?? null,
      authorityHeadSeq: context.authorityHeadSeq ?? 0,
      verifierRequired: context.verifierRequired === true,
    },
    includedEventIds: inputEventIds,
    excluded: [],
    conflicts: [],
    assignments,
    unseatedReports: [],
    notes: [],
    decisions,
    waitingOnDecision: waiting
      ? { requestId: waiting.requestId, heldOn: waiting.heldOn }
      : null,
    pendingCompletion: null,
    canonicalTerminal: null,
  };
}

export async function handleWaveBBadgesMockCommand(
  command: string,
  payload: unknown,
  _config: WaveBMockCommandConfig,
): Promise<WaveBMockCommandResult> {
  if (!enabled()) return null;
  const request = record(record(payload).request);
  if (command === CODING_SESSION_OBSERVATION_FOLD_COMMAND) {
    return { handled: true, value: foldObservations(request) };
  }
  if (command === CODING_SESSION_TEAM_FOLD_COMMAND) {
    return { handled: true, value: foldTeam(request) };
  }
  return null;
}
