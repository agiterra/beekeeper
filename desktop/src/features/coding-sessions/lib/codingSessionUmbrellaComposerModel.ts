/**
 * Pure model behind the umbrella composer's participant selector.
 *
 * v1 authority (design, "Operator authority"): the umbrella founder prompts
 * executions, along with any accepted operator from the session's authority
 * chain when the caller supplies the roster's live operator set; everyone
 * else is an observer for actuation. The
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

/** Advisory hint shown to non-operator members of a governed session. */
export const CODING_SESSION_VIEW_ONLY_REASON =
  "View only — ask the session owner for collaborator access";

/**
 * Resolve the current user's authority over an umbrella's executions.
 *
 * `acceptedOperators` is the roster fold's live operator set (see
 * `codingSessionRoster`). When it is provided (non-nullish), an accepted
 * operator may steer alongside the founder, and everyone else reads the
 * view-only hint. When absent — roster not yet loaded, or a caller that
 * never wires it — the historical founder-only rule applies unchanged.
 */
export function resolveCodingSessionUmbrellaComposerAuthority(input: {
  umbrella: Pick<CodingSessionUmbrellaRecord, "founderPubkey" | "genesisRef">;
  currentUserPubkey: string | null;
  acceptedOperators?: ReadonlySet<string> | null;
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
  if (input.acceptedOperators != null) {
    if (input.acceptedOperators.has(input.currentUserPubkey)) {
      return { canPromptExecutions: true, reason: null, isUngovernedSession };
    }
    // The roster is known and this user holds no operator grant: viewer (or
    // no grant at all) — the composer disables with the invite-shaped hint.
    return {
      canPromptExecutions: false,
      reason: CODING_SESSION_VIEW_ONLY_REASON,
      isUngovernedSession,
    };
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
