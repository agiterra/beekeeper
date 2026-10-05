/**
 * The surface badges read from one surface `ctx` (SV-22).
 *
 * The Badge components and the header's right-panel dot (B1) both call
 * these, so a badge in the launcher and the dot over the closed panel can
 * never disagree about what is happening. Pure: the two inputs a `ctx` does
 * not hold — the running gates (judged against a clock) and this device's
 * Diff marker — are passed in.
 */
import type {
  CodingSessionSurfaceCtx,
  CodingSessionSurfaceExecutionEntry,
} from "@/features/coding-sessions/ui/surfaces/codingSessionSurfaceContext";
import { truncatePubkey } from "@/shared/lib/pubkey";
import {
  CODING_SESSION_RUNNING_GATE_RESTART_NOTE,
  type CodingSessionRunningGate,
} from "./codingSessionObservationView";
import { deriveCodingSessionSubagentPanel } from "./codingSessionSubagents";
import {
  codingSessionFailedCommitlessGates,
  codingSessionFailedGatesOnHead,
  codingSessionNewestEditPerFile,
  codingSessionSurfaceOpenRulings,
  codingSessionUnseenDiffFiles,
  deriveCodingSessionAgentsBadge,
  deriveCodingSessionDiffBadge,
  deriveCodingSessionLandingBadge,
  deriveCodingSessionPlanBadge,
  readCodingSessionLandingBadgeExtension,
  type CodingSessionDiffSeenMarker,
  type CodingSessionNewestFileEdit,
  type CodingSessionSurfaceBadge,
  type CodingSessionSurfaceDecisionRow,
  type CodingSessionSurfaceOpenRuling,
} from "./codingSessionSurfaceBadgeModel";

/** The surfaces that carry a badge built here. Terminal's is B4's. */
export type CodingSessionBadgedSurfaceId =
  | "agents"
  | "diff"
  | "plan"
  | "landing";

/**
 * The ctx's decision rows — the team fold's `decisions[]`, verbatim — or
 * `null` while the session has no genesis or the fold has not been read:
 * unknown, never "none".
 */
export function codingSessionSurfaceDecisionsFromCtx(
  ctx: Pick<CodingSessionSurfaceCtx, "decisions">,
): readonly CodingSessionSurfaceDecisionRow[] | null {
  return ctx.decisions ?? null;
}

/**
 * Who a ruling is held on, in the decision queue's words (`heldOnLabel`):
 * `you` for the viewer, `the founder`, a resolved name, else the canonical
 * truncation.
 */
export function codingSessionSurfaceRulingHolderName(
  ctx: Pick<
    CodingSessionSurfaceCtx,
    "currentUserPubkey" | "founderPubkey" | "resolveActorName"
  >,
): (heldOn: string) => string {
  const viewer = ctx.currentUserPubkey?.trim().toLowerCase() ?? null;
  const founder = ctx.founderPubkey?.trim().toLowerCase() ?? null;
  return (heldOn) => {
    const party = heldOn === "founder" ? founder : heldOn.trim().toLowerCase();
    if (viewer !== null && party !== null && viewer === party) return "you";
    if (heldOn === "founder" || (founder !== null && party === founder)) {
      return "the founder";
    }
    const resolved = ctx.resolveActorName(heldOn)?.trim();
    return resolved ? resolved : truncatePubkey(heldOn);
  };
}

/** The open rulings (DB8) the ctx's decision rows hold. */
export function codingSessionSurfaceRulingsFromCtx(
  ctx: CodingSessionSurfaceCtx,
): CodingSessionSurfaceOpenRuling[] {
  return codingSessionSurfaceOpenRulings(
    codingSessionSurfaceDecisionsFromCtx(ctx),
    codingSessionSurfaceRulingHolderName(ctx),
  );
}

/**
 * Subagents running now: open Task calls in the active generation of each
 * execution that is on a turn (`Working`).
 *
 * The transcript alone keeps an open Task call "running" when the provider
 * abandoned its turn without a result; the stream reads it through the
 * turn's settlement (`settleCodingSessionSubagentSpawns`) and shows it
 * stopped. A seat that is not working has no live turn, so none of its
 * calls is live — the same gate the Plan badge applies. `Starting` is not a
 * turn either: nothing may settle a call as live under it (SV-43).
 */
export function codingSessionLiveSubagentCount(
  executions: readonly CodingSessionSurfaceExecutionEntry[],
): number {
  let running = 0;
  for (const entry of executions) {
    if (entry.status.kind !== "working" || entry.status.label !== "Working") {
      continue;
    }
    running += deriveCodingSessionSubagentPanel([
      entry.execution.activeGeneration.transcript,
    ]).running;
  }
  return running;
}

/**
 * Seats with one status word: `Working` (on a turn) or `Starting` (signed
 * `starting`, not on a turn yet). Both share the `working` kind, so the
 * label is what tells them apart (SV-43).
 */
function codingSessionSeatsLabelled(
  executions: readonly CodingSessionSurfaceExecutionEntry[],
  label: "Working" | "Starting",
): number {
  return executions.filter(
    (entry) => entry.status.kind === "working" && entry.status.label === label,
  ).length;
}

/**
 * Agents, from the ctx: live subagents, seats on a turn and seats starting
 * (umbrella), rulings. `liveSubagents` may be passed when the caller memoized
 * {@link codingSessionLiveSubagentCount}.
 */
export function codingSessionAgentsBadgeFromCtx(
  ctx: CodingSessionSurfaceCtx,
  liveSubagents?: number,
): CodingSessionSurfaceBadge | null {
  const umbrella = ctx.layout === "umbrella";
  return deriveCodingSessionAgentsBadge({
    runningSubagents:
      liveSubagents ?? codingSessionLiveSubagentCount(ctx.executions),
    workingSeats: umbrella
      ? codingSessionSeatsLabelled(ctx.executions, "Working")
      : 0,
    startingSeats: umbrella
      ? codingSessionSeatsLabelled(ctx.executions, "Starting")
      : 0,
    rulings: codingSessionSurfaceRulingsFromCtx(ctx),
  });
}

/** Plan, from the ctx: tasks in progress while the focused seat works. */
export function codingSessionPlanBadgeFromCtx(
  ctx: CodingSessionSurfaceCtx,
): CodingSessionSurfaceBadge | null {
  const focusedKey = ctx.focusedExecution?.executionKey ?? null;
  const focused =
    focusedKey === null
      ? null
      : ctx.executions.find(
          (entry) => entry.execution.executionKey === focusedKey,
        );
  return deriveCodingSessionPlanBadge({
    taskModel: ctx.taskModel,
    working: focused?.status.kind === "working",
  });
}

/** Clock time for a provider-clock instant, e.g. `14:02`. */
export function formatCodingSessionSurfaceClock(ms: number): string {
  return new Date(ms).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** Names the provider instance that watched a gate: its name, else its key. */
export function codingSessionSurfaceWatcherName(
  ctx: Pick<CodingSessionSurfaceCtx, "resolveActorName">,
): (pubkey: string) => string {
  return (pubkey) => {
    const resolved = ctx.resolveActorName(pubkey)?.trim();
    return resolved ? resolved : `provider ${truncatePubkey(pubkey)}`;
  };
}

/** Landing, from the ctx and the running gates the caller's clock judged. */
export function codingSessionLandingBadgeFromCtx(
  ctx: CodingSessionSurfaceCtx,
  runningGates: readonly CodingSessionRunningGate[],
  formatTime: (ms: number) => string = formatCodingSessionSurfaceClock,
): CodingSessionSurfaceBadge | null {
  const read = ctx.observations.state === "read" ? ctx.observations : null;
  return deriveCodingSessionLandingBadge({
    // As the Landing panel reads it: no fold read yet is "checked", since
    // there is no start to word either way.
    provenanceChecked: read?.result?.fold?.provenanceChecked ?? true,
    failedGates: read
      ? codingSessionFailedGatesOnHead(
          read.view.gates,
          read.result?.signedAt ?? null,
        )
      : [],
    failedCommitlessGates: read
      ? codingSessionFailedCommitlessGates(
          read.view.gates,
          read.result?.signedAt ?? null,
        )
      : [],
    refusingVerdict: readCodingSessionLandingBadgeExtension(
      ctx.extensions.landing,
    ),
    rulings: codingSessionSurfaceRulingsFromCtx(ctx),
    runningGates,
    runningGateNote: CODING_SESSION_RUNNING_GATE_RESTART_NOTE,
    nameWatcher: codingSessionSurfaceWatcherName(ctx),
    formatTime,
  });
}

/**
 * Diff, from the ctx and this device's marker. `newest` may be passed when
 * the caller already folded `ctx.transcript` (the Diff badge memoizes it).
 */
export function codingSessionDiffBadgeFromCtx(
  ctx: CodingSessionSurfaceCtx,
  marker: CodingSessionDiffSeenMarker | null,
  newest?: ReadonlyMap<string, CodingSessionNewestFileEdit>,
): CodingSessionSurfaceBadge | null {
  const onScreen = ctx.activeSurfaceId === "diff";
  if (onScreen || marker === null) {
    return deriveCodingSessionDiffBadge({
      unseenFiles: 0,
      via: marker?.via ?? null,
      onScreen,
    });
  }
  return deriveCodingSessionDiffBadge({
    unseenFiles: codingSessionUnseenDiffFiles(
      newest ?? codingSessionNewestEditPerFile([...ctx.transcript]),
      marker,
    ).length,
    via: marker.via,
    onScreen,
  });
}

/**
 * Every badge built here, by surface id — what B1's right-panel dot reads
 * with `strongestCodingSessionSurfaceBadgeTone` over the surfaces not on
 * screen.
 */
export function deriveCodingSessionSurfaceBadgesFromCtx(
  ctx: CodingSessionSurfaceCtx,
  input: {
    runningGates: readonly CodingSessionRunningGate[];
    diffMarker: CodingSessionDiffSeenMarker | null;
  },
): Record<CodingSessionBadgedSurfaceId, CodingSessionSurfaceBadge | null> {
  return {
    agents: codingSessionAgentsBadgeFromCtx(ctx),
    diff: codingSessionDiffBadgeFromCtx(ctx, input.diffMarker),
    plan: codingSessionPlanBadgeFromCtx(ctx),
    landing: codingSessionLandingBadgeFromCtx(ctx, input.runningGates),
  };
}
