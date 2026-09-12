/**
 * How each team wake actually reached the lead, and whether each seated
 * execution holds the seat its create claims.
 *
 * Both are pure functions of things that were signed or that Desktop itself
 * durably wrote: a 44224 receipt from the lead's own provider authority, a
 * lead user-prompt echo, the accepted 44228 chain, and Desktop's wake ledger.
 * Nothing here infers, and `unknown` is a real answer rather than a stand-in
 * for "no rows" — the whole point of the surface is that a person can tell
 * "the provider delivered it" from "we cannot see whether anyone did".
 *
 * The ordering of the delivery kinds is the §2a contract ruling in one place:
 * a provider command that reached `turn_queued` owns the operation, so it
 * outranks anything Desktop did or is about to do; `DUPLICATE_OPERATION` is
 * settlement rather than failure (I13); and `failed` requires both that every
 * known command failed for a non-duplicate reason *and* that Desktop's single
 * re-arm is already spent (I14).
 */
import type {
  CodingSessionSeatAuthority,
  CodingSessionSeatAuthorityKind,
  CodingSessionTeamWakeDelivery,
  CodingSessionTeamWakeDeliveryFailure,
  CodingSessionTeamWakeDeliveryKind,
} from "./codingSessionMissionContracts";
import {
  CODING_SESSION_DUPLICATE_OPERATION_CODE,
  CODING_SESSION_TEAM_WAKE_RESIDUAL_DETAIL,
  codingSessionSeatAuthorityCopy,
  codingSessionSeatRepairRemedy,
  codingSessionTeamWakeDeliveryCopy,
} from "./codingSessionMissionContracts";
import type { CodingSessionCommandTarget } from "./codingSessionCommand";
import type {
  CodingSessionTeamWakeCandidate,
  CodingSessionTeamWakeCustody,
  CodingSessionTeamWakeState,
} from "./codingSessionTeamWake";
import {
  CODING_SESSION_TEAM_WAKE_GRACE_MS,
  codingSessionTeamWakeCustodyFor,
  codingSessionTeamWakeFallbackNotBefore,
  codingSessionTeamWakePublishFailed,
  codingSessionTeamWakeReArmCount,
  codingSessionTeamWakeText,
} from "./codingSessionTeamWake";
import type { CodingSessionTeamWakeEvidenceIndex } from "./codingSessionTeamWakeEvidence";

/** Verified failure receipts disclosed per delivery row. */
const MAX_DELIVERY_FAILURES = 8;

/**
 * The runner's refusal for a sender that may not steer this execution.
 *
 * A command refused for this reason never held the operation, so it is not a
 * *delivery* that failed — it is a delivery that was never authorised. The
 * pointer text and the lead target key are both public, so any channel member
 * can publish one; letting it count as a failure would let a stranger spend
 * Desktop's single re-arm before Desktop had covered even once.
 */
const UNAUTHORIZED_OPERATOR_CODE = "UNAUTHORIZED_OPERATOR";

/** Kinds from which a Desktop publish may still legitimately follow. */
const PUBLISHABLE_KINDS: ReadonlySet<CodingSessionTeamWakeDeliveryKind> =
  new Set(["fallback-grace", "fallback-unconfirmed"]);

/**
 * Kinds that mean some command holds this operation right now, so Desktop must
 * not publish. Custody (`*-queued`) suppresses exactly as resolution does; the
 * difference is only whether the source is written to the permanent ledger.
 */
const SUPPRESSING_KINDS: ReadonlySet<CodingSessionTeamWakeDeliveryKind> =
  new Set([
    "provider-queued",
    "provider-started",
    "fallback-queued",
    "fallback-started",
  ]);

/** Kinds that mean the turn actually ran: permanent resolution. */
const RESOLVED_KINDS: ReadonlySet<CodingSessionTeamWakeDeliveryKind> = new Set([
  "provider-started",
  "fallback-started",
]);

/** Everything the delivery derivation reads. All of it signed or durable. */
export type CodingSessionTeamWakeDeliveryInput = {
  candidates: readonly CodingSessionTeamWakeCandidate[];
  leadTarget: CodingSessionCommandTarget;
  leadTargetKey: string;
  founderPubkey: string | null;
  index: CodingSessionTeamWakeEvidenceIndex | null;
  acknowledgedCommandIds: ReadonlySet<string>;
  acknowledgedSourceEventIds: ReadonlySet<string>;
  state: CodingSessionTeamWakeState;
  /** Local wall clock. The only time input, and only for the local grace (D7). */
  nowMs: number;
  evidenceComplete: boolean;
  /**
   * Sources whose Desktop publish threw on this mount. A throw is a failure
   * Desktop witnessed directly; it needs no receipt to be true.
   */
  publishFailedSourceEventIds?: ReadonlySet<string>;
};

/**
 * The delivery rows plus the two sets the wake hook acts on.
 *
 * They are returned together because they are one decision: a source is
 * settled exactly when its kind says somebody owns it, and re-arm-eligible
 * exactly when nothing does and the one allowed re-arm is unspent.
 */
export type CodingSessionTeamWakeDeliveryPlan = {
  deliveries: CodingSessionTeamWakeDelivery[];
  /**
   * Sources a command carried to `turn_started`, or that the lead echoed.
   * These are written to the permanent ledger and can never be published again.
   */
  resolvedSourceEventIds: ReadonlySet<string>;
  /** Sources some command currently holds at queued/degraded, with its id. */
  custodiedSources: readonly CodingSessionTeamWakeCustody[];
  /**
   * Sources whose recorded custodian has since been dropped or refused for a
   * non-duplicate reason without ever starting. The runner's fence is released,
   * so the custody row must be forgotten and the re-arm rule applies.
   */
  releasedCustodySourceEventIds: ReadonlySet<string>;
  /** Sources Desktop must not publish for right now: resolved or custodied. */
  suppressedSourceEventIds: ReadonlySet<string>;
  /**
   * Sources the hook may publish for right now.
   *
   * Deliberately **not** the same question as "is the row `failed`". A durable
   * `publishFailed` row makes the row read `failed` — which is true, Desktop's
   * publish threw — but it is disclosure, not a fence: the next mount must be
   * free to try again. Only a receipt-backed failure with the one re-arm
   * already spent, incomplete evidence, or a live custodian is terminal here.
   */
  publishEligibleSourceEventIds: ReadonlySet<string>;
  /** Sources whose single Desktop re-arm is now due. */
  reArmEligibleSourceEventIds: ReadonlySet<string>;
  /**
   * Command ids the runner answered `DUPLICATE_OPERATION`.
   *
   * That attempt is spent: republishing the same id would only be refused
   * again. It says nothing about the source, which stays publishable under a
   * *different* id if every live command later fails.
   */
  spentCommandIds: ReadonlySet<string>;
};

type CommandView = {
  commandId: string;
  fromProvider: boolean;
  createdAt: number;
  progress: "started" | "queued" | null;
  failure: CodingSessionTeamWakeDeliveryFailure | null;
  duplicate: boolean;
};

/** Desktop's own command ids for this source and lead target, from the ledger. */
function desktopLedgerCommandIds(
  input: CodingSessionTeamWakeDeliveryInput,
  candidate: CodingSessionTeamWakeCandidate,
): string[] {
  const owns = (row: { sourceEventId: string; leadTargetKey: string }) =>
    row.sourceEventId === candidate.sourceEventId &&
    row.leadTargetKey === input.leadTargetKey;
  return [
    ...input.state.pending.filter(owns).map((row) => row.commandId),
    ...input.state.reArmed.filter(owns).map((row) => row.commandId),
  ];
}

function commandViews(
  input: CodingSessionTeamWakeDeliveryInput,
  candidate: CodingSessionTeamWakeCandidate,
  pointerText: string,
): CommandView[] {
  const founder = input.founderPubkey?.toLowerCase() ?? null;
  const desktopIds = desktopLedgerCommandIds(input, candidate);
  const views = new Map<string, CommandView>();
  for (const command of input.index?.commandsFor(pointerText) ?? []) {
    const failure = input.index?.failureFor(command.commandId) ?? null;
    const duplicate = failure?.code === CODING_SESSION_DUPLICATE_OPERATION_CODE;
    const progress = input.index?.progressFor(command.commandId) ?? null;
    const desktopOwned =
      (founder !== null && command.signerPubkey === founder) ||
      desktopIds.includes(command.commandId);
    // A stranger's command that the runner refused as unauthorised is dropped
    // from the command set entirely: it neither failed, nor custodied, nor
    // counts toward "every known command has been spent".
    if (
      !desktopOwned &&
      progress === null &&
      failure?.outcome === "refused" &&
      failure.code === UNAUTHORIZED_OPERATOR_CODE
    ) {
      continue;
    }
    views.set(command.commandId, {
      commandId: command.commandId,
      // A founder-signed 44220 is Desktop's own cover; anything else is the
      // provider's own delivery. A foreign producer's failure is never proof
      // that another producer delivered, which is why the two are separated
      // before any kind is chosen.
      fromProvider: !desktopOwned,
      createdAt: command.createdAt,
      progress:
        progress === null
          ? null
          : // An injected input is inside a turn that is running; for this
            // delivery's purposes that is "started", not "queued".
            progress.stage === "started" || progress.stage === "injected"
            ? "started"
            : "queued",
      failure: failure
        ? {
            commandId: command.commandId,
            outcome: failure.outcome,
            code: failure.code,
            message: failure.message,
          }
        : null,
      duplicate,
    });
  }
  // Desktop's own accepted publishes are known from the durable ledger even
  // before any receipt exists — that is exactly what `fallback-unconfirmed`
  // reports.
  for (const commandId of desktopIds) {
    const existing = views.get(commandId);
    if (existing) {
      existing.fromProvider = false;
      continue;
    }
    // Desktop knows its own command id, so a receipt about it is evidence
    // even when the 44220 itself never came back through the subscription —
    // which is the ordinary case, since Desktop does not re-read what it sent.
    const failure = input.index?.failureFor(commandId) ?? null;
    const progress = input.index?.progressFor(commandId) ?? null;
    views.set(commandId, {
      commandId,
      fromProvider: false,
      createdAt: 0,
      progress:
        progress === null
          ? null
          : // An injected input is inside a turn that is running; for this
            // delivery's purposes that is "started", not "queued".
            progress.stage === "started" || progress.stage === "injected"
            ? "started"
            : "queued",
      failure: failure
        ? {
            commandId,
            outcome: failure.outcome,
            code: failure.code,
            message: failure.message,
          }
        : null,
      duplicate: failure?.code === CODING_SESSION_DUPLICATE_OPERATION_CODE,
    });
  }
  return [...views.values()];
}

function echoed(
  input: CodingSessionTeamWakeDeliveryInput,
  candidate: CodingSessionTeamWakeCandidate,
  view: CommandView,
): boolean {
  return (
    input.acknowledgedCommandIds.has(view.commandId) ||
    input.acknowledgedSourceEventIds.has(candidate.sourceEventId)
  );
}

/**
 * Whether this command's own receipts say it will never run.
 *
 * A non-duplicate `turn_dropped`/`turn_refused` with no start is exactly the
 * condition under which the runner's operation fence releases (00-BATCH §3),
 * so a command in that state stops being a custodian even though an earlier
 * `turn_queued` for it is still on the wire. A `DUPLICATE_OPERATION` refusal is
 * settlement rather than a drop and never releases anything by itself.
 */
function releasesCustody(view: CommandView): boolean {
  return (
    view.failure !== null && !view.duplicate && view.progress !== "started"
  );
}

function deliveryDetail(
  kind: CodingSessionTeamWakeDeliveryKind,
  failures: readonly CodingSessionTeamWakeDeliveryFailure[],
  views: readonly CommandView[],
): string {
  // The disclosed §2a residual: every failure we can see is the lead dropping
  // the turn, and at least one of those drops is on a command Desktop did not
  // send — the lead died holding the wake and nobody was there to cover.
  //
  // `!view.duplicate` is load-bearing. A `DUPLICATE_OPERATION` refusal is
  // settlement, not a drop; counting it here made the row say "the lead dropped
  // it and no Desktop was present" in the exact case where the lead dropped
  // nothing and a Desktop had covered twice.
  if (
    kind === "failed" &&
    failures.length > 0 &&
    failures.every((failure) => failure.outcome === "dropped") &&
    views.some(
      (view) =>
        view.fromProvider &&
        !view.duplicate &&
        view.failure?.outcome === "dropped",
    )
  ) {
    return CODING_SESSION_TEAM_WAKE_RESIDUAL_DETAIL;
  }
  return codingSessionTeamWakeDeliveryCopy[kind].detail;
}

/**
 * Derive one delivery row per candidate, plus the settlement and re-arm sets
 * the wake hook acts on.
 *
 * Deliberately **not** truncated: the hook suppresses publishes from these
 * rows, so dropping the oldest here would let an old source publish a second
 * command. Rendering bounds belong to the surface, which has
 * `CODING_SESSION_MISSION_DELIVERY_ROW_LIMIT` for exactly that.
 */
export function deriveCodingSessionTeamWakeDeliveryPlan(
  input: CodingSessionTeamWakeDeliveryInput,
): CodingSessionTeamWakeDeliveryPlan {
  const resolved = new Set<string>();
  const suppressed = new Set<string>();
  const releasedCustody = new Set<string>();
  const custodied: CodingSessionTeamWakeCustody[] = [];
  const reArmEligible = new Set<string>();
  const publishEligible = new Set<string>();
  const spentCommandIds = new Set<string>();
  const deliveries = input.candidates.map((candidate) => {
    const pointerText = codingSessionTeamWakeText(candidate);
    const views = commandViews(input, candidate, pointerText);
    const reArmCount = codingSessionTeamWakeReArmCount(
      input.state,
      candidate.sourceEventId,
      input.leadTargetKey,
    );
    const duplicateRefusedCommandIds = views
      .filter((view) => view.duplicate)
      .map((view) => view.commandId);
    for (const commandId of duplicateRefusedCommandIds) {
      spentCommandIds.add(commandId);
    }
    const failures = views
      .filter((view) => view.failure !== null && !view.duplicate)
      .sort((left, right) => right.createdAt - left.createdAt)
      .slice(0, MAX_DELIVERY_FAILURES)
      .map((view) => view.failure as CodingSessionTeamWakeDeliveryFailure);
    // A durable custody row is evidence in its own right: the receipt that
    // created it may have aged out of the relay's window, and forgetting it
    // would republish. It is released only by its own command's non-duplicate
    // failure — the moment the runner's fence releases (00-BATCH §3).
    const recordedCustody = codingSessionTeamWakeCustodyFor(
      input.state,
      candidate.sourceEventId,
      input.leadTargetKey,
    );
    const liveCustody = recordedCustody.filter((row) => {
      const view = views.find((item) => item.commandId === row.commandId);
      return !view || !releasesCustody(view);
    });
    if (recordedCustody.length > 0 && liveCustody.length === 0) {
      releasedCustody.add(candidate.sourceEventId);
    }
    const providerStarter = views.find(
      (view) => view.fromProvider && view.progress === "started",
    );
    const desktopStarter = views.find(
      (view) => !view.fromProvider && view.progress === "started",
    );
    const providerEcho = views.find(
      (view) => view.fromProvider && echoed(input, candidate, view),
    );
    const desktopEcho = views.find(
      (view) => !view.fromProvider && echoed(input, candidate, view),
    );
    // A lead echo of the pointer proves a turn ran for it, whoever sent the
    // command — even one this client never saw on the wire.
    const bareEcho =
      input.acknowledgedSourceEventIds.has(candidate.sourceEventId) &&
      views.length === 0;
    const providerResolver = providerStarter ?? providerEcho;
    const desktopResolver = desktopStarter ?? desktopEcho;
    const providerCustodian = views.find(
      (view) =>
        view.fromProvider &&
        view.progress === "queued" &&
        !releasesCustody(view),
    );
    const desktopCustodian = views.find(
      (view) =>
        !view.fromProvider &&
        view.progress === "queued" &&
        !releasesCustody(view),
    );
    const recordedCustodian =
      liveCustody.length > 0
        ? {
            commandId: liveCustody[0].commandId,
            // The row's own recorded origin is the answer when the command has
            // no view left — which is exactly the case a re-derivation gets
            // wrong, since the ledger it would consult is what was evicted.
            fromProvider:
              views.find((view) => view.commandId === liveCustody[0].commandId)
                ?.fromProvider ?? liveCustody[0].fromProvider,
          }
        : null;
    const fallbackNotBeforeMs = codingSessionTeamWakeFallbackNotBefore(
      input.state,
      candidate.sourceEventId,
    );
    const publishThrew =
      (input.publishFailedSourceEventIds?.has(candidate.sourceEventId) ??
        false) ||
      codingSessionTeamWakePublishFailed(
        input.state,
        candidate.sourceEventId,
        input.leadTargetKey,
      );
    // §2 (d): every known command has spent itself — each is either a
    // non-duplicate failure or a duplicate refusal — and at least one really
    // failed. A duplicate refusal alone proves only that custody lay elsewhere.
    const allFailed =
      views.length > 0 &&
      views.every((view) => view.failure !== null) &&
      failures.length > 0;
    const kind = deliveryKind({
      evidenceComplete: input.evidenceComplete,
      providerResolver: Boolean(providerResolver) || bareEcho,
      desktopResolver: Boolean(desktopResolver),
      providerCustodian: Boolean(
        providerCustodian ?? (recordedCustodian?.fromProvider ? true : null),
      ),
      desktopCustodian: Boolean(
        desktopCustodian ??
          (recordedCustodian && !recordedCustodian.fromProvider ? true : null),
      ),
      allFailed,
      reArmCount,
      publishThrew,
      hasDesktopCommand: views.some(
        (view) => !view.fromProvider && view.failure === null,
      ),
    });
    if (RESOLVED_KINDS.has(kind)) resolved.add(candidate.sourceEventId);
    if (SUPPRESSING_KINDS.has(kind)) suppressed.add(candidate.sourceEventId);
    // Eligibility is the same ladder with the publish-failure disclosure
    // removed. Everything else that makes a kind terminal — a spent re-arm on
    // receipt-backed failures, a live custodian, incomplete evidence — still
    // does.
    const eligibleKind = deliveryKind({
      evidenceComplete: input.evidenceComplete,
      providerResolver: Boolean(providerResolver) || bareEcho,
      desktopResolver: Boolean(desktopResolver),
      providerCustodian: Boolean(
        providerCustodian ?? (recordedCustodian?.fromProvider ? true : null),
      ),
      desktopCustodian: Boolean(
        desktopCustodian ??
          (recordedCustodian && !recordedCustodian.fromProvider ? true : null),
      ),
      allFailed,
      reArmCount,
      publishThrew: false,
      hasDesktopCommand: views.some(
        (view) => !view.fromProvider && view.failure === null,
      ),
    });
    if (PUBLISHABLE_KINDS.has(eligibleKind)) {
      publishEligible.add(candidate.sourceEventId);
    }
    const custodian =
      providerCustodian ?? desktopCustodian ?? recordedCustodian ?? null;
    if (!RESOLVED_KINDS.has(kind) && SUPPRESSING_KINDS.has(kind) && custodian) {
      custodied.push({
        sourceEventId: candidate.sourceEventId,
        leadTargetKey: input.leadTargetKey,
        commandId: custodian.commandId,
        fromProvider: custodian.fromProvider,
      });
    }
    // Desktop's *first* cover for a source is never the re-arm: it uses the
    // base id, and the one-shot `:r1` is held back for the case the re-arm
    // exists to serve — a command Desktop itself already spent. Without this,
    // any stranger's failed 44220 would make `allFailed` true and burn the
    // second chance before Desktop had published once.
    // A publish that *threw* is not a Desktop-owned command: nothing reached
    // the relay, so nothing was spent and the next cover is still a first one.
    const desktopOwnedExists = views.some((view) => !view.fromProvider);
    // A duplicate refusal never vetoes the re-arm. The command it names is the
    // custodian, and that custodian's own receipt is what suppresses; when the
    // custodian is later dropped, Desktop's one cover is due.
    if (
      input.evidenceComplete &&
      allFailed &&
      reArmCount === 0 &&
      desktopOwnedExists &&
      !SUPPRESSING_KINDS.has(kind)
    ) {
      reArmEligible.add(candidate.sourceEventId);
    }
    const owner =
      providerResolver ??
      providerCustodian ??
      desktopResolver ??
      desktopCustodian ??
      recordedCustodian ??
      null;
    return {
      sourceEventId: candidate.sourceEventId,
      operationType:
        candidate.kind === "operation_ready"
          ? ("report" as const)
          : ("terminal" as const),
      sourceActorPubkey: candidate.sourceActorPubkey,
      leadTargetKey: input.leadTargetKey,
      kind,
      owningCommandId: kind === "unknown" ? null : (owner?.commandId ?? null),
      duplicateRefusedCommandIds,
      failures,
      reArmCount,
      observedAtMs:
        fallbackNotBeforeMs === null
          ? null
          : fallbackNotBeforeMs - CODING_SESSION_TEAM_WAKE_GRACE_MS,
      detail: deliveryDetail(kind, failures, views),
    } satisfies CodingSessionTeamWakeDelivery;
  });
  return {
    deliveries,
    resolvedSourceEventIds: resolved,
    custodiedSources: custodied,
    releasedCustodySourceEventIds: releasedCustody,
    suppressedSourceEventIds: suppressed,
    publishEligibleSourceEventIds: publishEligible,
    reArmEligibleSourceEventIds: reArmEligible,
    spentCommandIds,
  };
}

function deliveryKind(input: {
  evidenceComplete: boolean;
  providerResolver: boolean;
  desktopResolver: boolean;
  providerCustodian: boolean;
  desktopCustodian: boolean;
  allFailed: boolean;
  reArmCount: 0 | 1;
  publishThrew: boolean;
  hasDesktopCommand: boolean;
}): CodingSessionTeamWakeDeliveryKind {
  // Incomplete evidence is never rendered as any other kind: an index that is
  // still loading, errored, or saturated cannot distinguish "nobody delivered"
  // from "we cannot see who did".
  if (!input.evidenceComplete) return "unknown";
  if (input.providerResolver) return "provider-started";
  if (input.desktopResolver) return "fallback-started";
  if (input.providerCustodian) return "provider-queued";
  if (input.desktopCustodian) return "fallback-queued";
  if (input.publishThrew) return "failed";
  if (input.allFailed && input.reArmCount === 1) return "failed";
  if (input.hasDesktopCommand) return "fallback-unconfirmed";
  return "fallback-grace";
}

/** The delivery rows alone, for surfaces that do not act on settlement. */
export function deriveCodingSessionTeamWakeDeliveries(
  input: CodingSessionTeamWakeDeliveryInput,
): CodingSessionTeamWakeDelivery[] {
  return deriveCodingSessionTeamWakeDeliveryPlan(input).deliveries;
}

/** The lookup key pairing one actor with one exact role. */
export function codingSessionSeatGrantKey(
  actorPubkey: string,
  role: string,
): string {
  return `${actorPubkey} ${role}`;
}

/** The accepted 44228 seat facts this derivation is allowed to read. */
export type CodingSessionSeatAuthorityProjection = {
  activeSeats: readonly { actorPubkey: string; role: string }[];
  /** `codingSessionSeatGrantKey` → the accepted `grant-seat` event id. */
  seatGrantRefs: Readonly<Record<string, string>>;
};

type SeatAuthorityExecution = {
  executionKey: string;
  activeGeneration: { agentRef: string | null; role: string | null };
};

/**
 * Seat authority for every receipt-backed seated execution.
 *
 * TypeScript decides nothing here beyond an exact (actor, role) match against
 * the accepted chain the relay already receipted (I6). A null projection is
 * `unknown` — the honest answer while the chain has not loaded — and never
 * "ungranted", which would accuse a seat of something no evidence shows.
 */
export function deriveCodingSessionSeatAuthorities(input: {
  channelId: string;
  umbrella: {
    sessionRef: string | null;
    executions: readonly SeatAuthorityExecution[];
  };
  authority: CodingSessionSeatAuthorityProjection | null;
}): CodingSessionSeatAuthority[] {
  const seats = new Set(
    (input.authority?.activeSeats ?? []).map((seat) =>
      codingSessionSeatGrantKey(seat.actorPubkey, seat.role),
    ),
  );
  return input.umbrella.executions.flatMap((execution) => {
    const actorPubkey = execution.activeGeneration.agentRef;
    const role = execution.activeGeneration.role;
    if (!actorPubkey || !role) return [];
    const seatKey = codingSessionSeatGrantKey(actorPubkey, role);
    const kind: CodingSessionSeatAuthorityKind =
      input.authority === null
        ? "unknown"
        : seats.has(seatKey)
          ? "granted"
          : "created-ungranted";
    return [
      {
        executionKey: execution.executionKey,
        actorPubkey,
        role,
        kind,
        grantEventId:
          kind === "granted"
            ? (input.authority?.seatGrantRefs[seatKey] ?? null)
            : null,
        detail: codingSessionSeatAuthorityCopy[kind].detail,
        remedy:
          kind === "created-ungranted" && input.umbrella.sessionRef
            ? codingSessionSeatRepairRemedy({
                channelId: input.channelId,
                sessionRef: input.umbrella.sessionRef,
                actorPubkey,
              })
            : null,
      },
    ];
  });
}
