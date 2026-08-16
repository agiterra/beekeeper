/**
 * Pure model behind the umbrella composer's participant selector.
 *
 * v1 authority (design, "Operator authority"): only the umbrella founder
 * prompts executions; everyone else is an observer for actuation. The
 * conversation lane is ordinary channel chat and stays open to any member.
 * Enforcement is client preflight — the relay stays a validating store — so
 * gating only ever *disables honestly*. A genesis-bearing session fails closed
 * until its founder and the current identity resolve. A legacy session with no
 * genesis retains the Step-3 membership fallback and is visibly ungoverned.
 *
 * The founder arrives from {@link useCodingSessionCreateObservations} via the
 * catalog snapshot, and only ever from a create the provider's own receipt
 * bound to an execution. That collection is asynchronous and can legitimately
 * come up empty (cold start, relay gap, disputed commandId). Whether that gap
 * restricts controls is determined by the explicit genesis reference, never
 * by guessing authority from relay history.
 */
import type { CodingSessionUmbrellaParticipant } from "./codingSessionUmbrellaModel";
import type { CodingSessionUmbrellaRecord } from "./codingSessionTypes";

export type CodingSessionUmbrellaComposerAuthority = {
  /** Whether this user may address executions (44220 turn/interrupt). */
  canPromptExecutions: boolean;
  /** Honest reason shown on disabled execution targets, when gated. */
  reason: string | null;
  /** Legacy sessions have no signed umbrella authority anchor yet. */
  isUngovernedSession: boolean;
};

/** Resolve the current user's v1 authority over an umbrella's executions. */
export function resolveCodingSessionUmbrellaComposerAuthority(input: {
  umbrella: Pick<CodingSessionUmbrellaRecord, "founderPubkey" | "genesisRef">;
  currentUserPubkey: string | null;
}): CodingSessionUmbrellaComposerAuthority {
  const founder = input.umbrella.founderPubkey;
  const isUngovernedSession = input.umbrella.genesisRef === null;
  if (founder === null) {
    if (isUngovernedSession) {
      return { canPromptExecutions: true, reason: null, isUngovernedSession };
    }
    return {
      canPromptExecutions: false,
      reason:
        "Session authority could not be resolved from its genesis. Controls are disabled until authority is available.",
      isUngovernedSession,
    };
  }
  if (input.currentUserPubkey === null) {
    return {
      canPromptExecutions: isUngovernedSession,
      reason: isUngovernedSession
        ? null
        : "Session authority is loading. Controls remain disabled until your identity is available.",
      isUngovernedSession,
    };
  }
  if (founder === input.currentUserPubkey) {
    return { canPromptExecutions: true, reason: null, isUngovernedSession };
  }
  return {
    canPromptExecutions: false,
    reason:
      "Only the session founder can prompt executions in this version. The session lane stays open to every member.",
    isUngovernedSession,
  };
}

/** Stable key for a selector entry. */
export function codingSessionUmbrellaParticipantKey(
  participant: CodingSessionUmbrellaParticipant,
): string {
  return participant.kind === "execution"
    ? `execution:${participant.executionKey}`
    : "session";
}

/**
 * The initial selection: the most recently active execution (design §C —
 * sticky last-addressed, initialized to the most recently active). Never the
 * Session entry, so the default send path is always today's 44220 path.
 */
export function defaultCodingSessionUmbrellaParticipantKey(
  participants: readonly CodingSessionUmbrellaParticipant[],
): string | null {
  let best: { key: string; lastEventMs: number } | null = null;
  for (const participant of participants) {
    if (participant.kind !== "execution") continue;
    const parsed = Date.parse(
      participant.execution.activeGeneration.lastEventAt,
    );
    const lastEventMs = Number.isFinite(parsed) ? parsed : 0;
    if (best === null || lastEventMs > best.lastEventMs) {
      best = {
        key: codingSessionUmbrellaParticipantKey(participant),
        lastEventMs,
      };
    }
  }
  return best?.key ?? null;
}
