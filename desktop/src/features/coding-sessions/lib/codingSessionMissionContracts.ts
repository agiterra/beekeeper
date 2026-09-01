/**
 * Frozen cross-lane contracts for the 2026-09-01 team-turn reliability and
 * Mission UI batch.
 *
 * Lane D (Desktop arbitration) implements the functions that produce these
 * values; Lane U (Mission UI) renders them; the finalizer wires them through
 * `CodingSessionUmbrellaWorkspace`. Member names and the copy in
 * `codingSessionTeamWakeDeliveryCopy` / `codingSessionSeatAuthorityCopy` are
 * the vocabulary the Inspector, the participant chips, and the stream rows all
 * share — one word per fact, everywhere. **Only the finalizer edits this file.**
 *
 * Everything here is a rendering of signed evidence or a local durable record:
 * a 44224 receipt signed by the lead execution's provider authority, a lead
 * user-prompt echo, the accepted 44228 seat chain, the Rust fold's
 * `unseatedReports`, or Desktop's own wake ledger. `unknown` is a first-class
 * value and is never rendered as any other kind.
 */

/**
 * `turn_refused` code the lead runner publishes when a second 44220 carries an
 * identifier-only team-wake pointer already custodied or consumed for the same
 * exact target. The refused command spent no turn. **Both producers treat a
 * `DUPLICATE_OPERATION` refusal of their own command as settlement of the
 * operation, never as a failure.**
 */
export const CODING_SESSION_DUPLICATE_OPERATION_CODE = "DUPLICATE_OPERATION";

/** How one operation wake reached (or failed to reach) the lead. */
export type CodingSessionTeamWakeDeliveryKind =
  | "provider-queued"
  | "provider-started"
  | "fallback-grace"
  | "fallback-unconfirmed"
  | "fallback-queued"
  | "fallback-started"
  | "failed"
  | "unknown";

/** A verified failure receipt on one command for this operation. */
export type CodingSessionTeamWakeDeliveryFailure = {
  commandId: string;
  outcome: "dropped" | "refused";
  code: string;
  message: string;
};

/** Delivery state of one operation source toward one lead target. */
export type CodingSessionTeamWakeDelivery = {
  /** The 44244 report event id, or the terminal transcript event id. */
  sourceEventId: string;
  operationType: "report" | "terminal";
  /** Actor pubkey that authored the source (the reporting seat), when known. */
  sourceActorPubkey: string | null;
  /** `buildCodingSessionTargetKey` of the lead generation this delivery addresses. */
  leadTargetKey: string;
  kind: CodingSessionTeamWakeDeliveryKind;
  /** The command currently owning the operation, when one is known. */
  owningCommandId: string | null;
  /** Commands answered `DUPLICATE_OPERATION` for this pointer + target — disclosure, never a failure. */
  duplicateRefusedCommandIds: readonly string[];
  /** Verified non-duplicate failure receipts, newest first, bounded to 8. */
  failures: readonly CodingSessionTeamWakeDeliveryFailure[];
  /** Desktop re-arm publishes already spent for this source + target. Exactly one is ever allowed. */
  reArmCount: 0 | 1;
  /** Local wall clock (ms) at which this kind was first observed; null when unknown. */
  observedAtMs: number | null;
  /** One sentence from `codingSessionTeamWakeDeliveryCopy`, possibly with the §2a residual clause. */
  detail: string;
};

/** Normative copy for each delivery kind. Render `detail`, never re-derive. */
export const codingSessionTeamWakeDeliveryCopy: Readonly<
  Record<
    CodingSessionTeamWakeDeliveryKind,
    { detail: string; badge: string | null }
  >
> = {
  "provider-queued": { detail: "Provider wake queued", badge: "queued" },
  "provider-started": { detail: "Provider wake started", badge: null },
  "fallback-grace": {
    detail: "Waiting for the provider wake",
    badge: "waiting",
  },
  "fallback-unconfirmed": {
    detail: "Desktop covered for the provider",
    badge: "fallback",
  },
  "fallback-queued": {
    detail: "Desktop covered for the provider",
    badge: "fallback",
  },
  "fallback-started": {
    detail: "Desktop covered for the provider",
    badge: "fallback",
  },
  failed: { detail: "Wake delivery failed", badge: "failed" },
  unknown: { detail: "Wake delivery unknown", badge: "unknown" },
};

/**
 * The disclosed §2a residual: the lead dropped a queued provider wake and no
 * Desktop was present to cover, so the wake is lost until either returns.
 */
export const CODING_SESSION_TEAM_WAKE_RESIDUAL_DETAIL =
  "Wake delivery failed — the lead dropped it and no Desktop was present to cover";

/** Whether a seated execution holds the governed seat its create claims. */
export type CodingSessionSeatAuthorityKind =
  | "granted"
  | "created-ungranted"
  | "unknown";

/** Seat authority for one execution, from the accepted 44228 chain. */
export type CodingSessionSeatAuthority = {
  executionKey: string;
  actorPubkey: string | null;
  role: string | null;
  kind: CodingSessionSeatAuthorityKind;
  /** Accepted `grant-seat` event id when `granted`. */
  grantEventId: string | null;
  /** One sentence from `codingSessionSeatAuthorityCopy`. */
  detail: string;
  /** Exact remedy command when `created-ungranted`; null otherwise. */
  remedy: string | null;
};

/** Normative copy for seat authority. */
export const codingSessionSeatAuthorityCopy: Readonly<
  Record<
    CodingSessionSeatAuthorityKind,
    { detail: string; badge: string | null }
  >
> = {
  granted: { detail: "Seat granted", badge: null },
  "created-ungranted": {
    detail: "Seat created, not granted",
    badge: "ungranted",
  },
  unknown: { detail: "Seat authority unknown", badge: "unknown" },
};

/** Build the exact repair command for a created-but-ungranted seat. */
export function codingSessionSeatRepairRemedy(input: {
  channelId: string;
  sessionRef: string;
  actorPubkey: string;
}): string {
  return `bee sessions seat-repair --channel ${input.channelId} --session-ref ${input.sessionRef} --actor ${input.actorPubkey}`;
}

/** Closed set of signed 44244 operation types the stream renders as rows. */
export type CodingSessionMissionTransactionType =
  | "assignment"
  | "report"
  | "refutation"
  | "disposition"
  | "acknowledgement"
  | "mission.completed"
  | "mission.blocked";

/**
 * One canonical (fold-included) 44244 transaction, projected for the stream.
 * Produced by Lane D's projection from the Rust fold; consumed by Lane U's row
 * builder. Nothing here is inferred from prose — every field is a signed body
 * field or a fold fact.
 */
export type CodingSessionMissionTransactionInput = {
  sourceEventId: string;
  type: CodingSessionMissionTransactionType;
  authorPubkey: string;
  /** Unix seconds from the signed event. */
  createdAt: number;
  /**
   * The other party: the assignee actor for an assignment; the assignment's
   * assignee for a report/acknowledgement author's counterpart (the assigner);
   * the report author for a verdict; null for terminals.
   */
  counterpartyPubkey: string | null;
  /** The parent it references (assignmentRef / reportRef / verdictRef), or null. */
  parentEventId: string | null;
  /** Bounded summary (≤ 280 chars) taken verbatim from the signed body. */
  summary: string;
  /** Verdict decision word, verbatim from the signed body; null otherwise. */
  decision: string | null;
  /** Required action, verbatim from the signed body; null otherwise. */
  requiredAction: string | null;
  /** File count from a report body; null otherwise. */
  fileCount: number | null;
  /** Structured test count from a report body; null otherwise. */
  testCount: number | null;
  /** True when the Rust fold lists this report under `unseatedReports`. */
  unseated: boolean;
};

/** Wire shape of one Rust-fold `unseatedReports` row. */
export type CodingSessionNativeUnseatedReport = {
  readonly eventId: string;
  readonly authorPubkey: string;
  readonly assignmentRef: string;
  readonly assigneeRole: string;
};

/** Bound for rendered delivery rows in the Inspector's Integrity section. */
export const CODING_SESSION_MISSION_DELIVERY_ROW_LIMIT = 32;
/** Bound for rendered transaction rows per stream (older rows collapse with a count). */
export const CODING_SESSION_MISSION_TRANSACTION_ROW_LIMIT = 200;
