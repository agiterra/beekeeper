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

/**
 * A verified terminal receipt on one command for this operation that is not
 * a run the provider can vouch for.
 *
 * `"unknown"` is a native steer whose delivery could not be established
 * (`turn_delivery_unknown`). A team wake is always a boundary turn, so this
 * arm is not expected here — but the index reports it truthfully rather than
 * folding it into `"dropped"`, which would tell a re-arm the words never ran.
 */
export type CodingSessionTeamWakeDeliveryFailure = {
  commandId: string;
  outcome: "dropped" | "refused" | "unknown";
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

/**
 * Closed set of signed 44244 operation types the stream renders as rows.
 *
 * `refutation` and `disposition` are the two `verdict` subtypes, split here
 * because the surface words them apart; every other member is a wire `type`
 * verbatim.
 *
 * Batch 1 froze this at seven members. **Amended for B1c**, which shipped
 * `note`, `decision.request` and `decision.answer` on the wire
 * (`codingSessionTeamTransactionWire.ts`) without widening it — so the type
 * claimed a session could not carry those rows while the projection was
 * already building them, and every table keyed on it came up `undefined`. The
 * freeze protects the vocabulary from drift, not the contract from the truth:
 * when the wire gains a verb the stream renders, this union gains it too, in
 * the same change.
 */
export type CodingSessionMissionTransactionType =
  | "assignment"
  | "report"
  | "refutation"
  | "disposition"
  | "acknowledgement"
  | "mission.completed"
  | "mission.blocked"
  | "note"
  | "decision.request"
  | "decision.answer";

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
  /**
   * A `decision.answer` row's signed `condition` — the class the ruling
   * covers — verbatim, or null when the ruling named none.
   *
   * Text, never a predicate: nothing evaluates it and no surface derives state
   * from it. Optional because the projection that fills it lands with the lane
   * that owns it; absent reads as "no condition", exactly as an answer that
   * named none does.
   */
  condition?: string | null;
  /**
   * An `assignment` row's signed `assigneeRole`, verbatim; null on every other
   * type. Optional because the projection that fills it lands with the lane
   * that owns it, and absent reads as "not known", never as a role.
   */
  assigneeRole?: string | null;
  /**
   * An `assignment` row's signed `baseSha` — the revision the assignee is to
   * start from — or null when the assignment named none.
   *
   * Required on the wire for a `verifier` or `runner` assignment and optional
   * for every other role, so `null` here on a verifier row is an older signed
   * body, never a revision this surface may invent.
   */
  baseSha?: string | null;
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

/**
 * Frozen copy for fold exclusion codes that a person, not a protocol reader,
 * has to act on (§1k, batch 3 lane L7).
 *
 * The Integrity list otherwise renders the adapter's raw wire token beside the
 * fold's own reason. That is honest but unreadable: `completion_not_verified`
 * says nothing to the founder who has to decide what to do about it. A code
 * with an entry here renders as `{word} · {detail}`; a code without one keeps
 * the raw token, because inventing copy for a code nobody wrote copy for would
 * be worse than showing the token.
 *
 * The fold's `reason` is never replaced — it stays as the row's evidence,
 * naming the exact assignment and report ids.
 */
/**
 * The adapter's wire token for a completion the fold refused for want of a
 * verifier's ruling.
 *
 * Named once so the copy table and the state line key off the same string; it
 * is `CodingSessionTeamFoldExclusionCode::CompletionNotVerified` as the Tauri
 * adapter serialises it.
 */
export const COMPLETION_NOT_VERIFIED_CODE = "completion_not_verified";

export const codingSessionFoldExclusionCopy: Readonly<
  Record<string, { readonly word: string; readonly detail: string }>
> = Object.freeze({
  [COMPLETION_NOT_VERIFIED_CODE]: Object.freeze({
    word: "Completion not verified",
    detail: "the policy requires a verifier's ruling",
  }),
});

/**
 * The Mission state line when the canonical terminal is missing because the
 * completion was excluded `completion_not_verified`.
 *
 * Rendered instead of `Running`: a mission whose completion the fold refused
 * is not running, and saying so is the difference between a status and a
 * guess. The state panel's longer sentence is §1l's and belongs to the lane
 * that owns that panel; this is the one-line form.
 */
export const CODING_SESSION_COMPLETION_REFUSED_STATE_LINE =
  "Completion refused · no verifier ruling";
