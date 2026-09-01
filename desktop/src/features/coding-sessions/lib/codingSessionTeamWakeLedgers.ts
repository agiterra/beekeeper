/**
 * The three durable side-ledgers of the team-wake state.
 *
 * Split out of `codingSessionTeamWake.ts` when that file reached the
 * repository's 1,000-line ceiling. They are grouped because they answer one
 * question between them — what has already happened to this source against
 * this lead generation — and because each is a *record*, never a decision:
 *
 * - `custodied` says which command currently holds the operation, so a
 *   remount whose relay window no longer returns the receipt still suppresses.
 * - `reArmed` says the one Desktop re-arm is spent.
 * - `publishFailed` says a Desktop publish threw. It is disclosure only: the
 *   next mount retries, and a success clears it.
 */
import type { CodingSessionTeamWakeState } from "./codingSessionTeamWake";

/** Bound for the custody ledger, matching the pending-attempt bound. */
export const MAX_CUSTODIED_WAKES = 128;
/** Bound for the durable publish-failure ledger. */
export const MAX_PUBLISH_FAILURES = 128;
/** Bound for the spent-re-arm ledger. */
export const MAX_RE_ARMED_WAKES = 128;
/** Maximum UTF-8 byte length of a command id or target key in these ledgers. */
export const MAX_LEDGER_TEXT_BYTES = 256;

const HEX64 = /^[0-9a-f]{64}$/;

/** True for a lowercase 64-character hex event id. */
export function isLedgerEventId(value: unknown): value is string {
  return typeof value === "string" && HEX64.test(value);
}

/** True for a bounded, single-line, non-empty identifier. */
export function isLedgerText(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    ![...value].some((character) => {
      const codePoint = character.codePointAt(0) ?? 0;
      return codePoint < 32 || codePoint === 127;
    }) &&
    new TextEncoder().encode(value).length <= MAX_LEDGER_TEXT_BYTES
  );
}

/** One spent Desktop re-arm. At most one ever exists per source and target. */
export type CodingSessionTeamWakeReArm = {
  sourceEventId: string;
  leadTargetKey: string;
  commandId: string;
};

/**
 * One command currently holding an operation at `turn_queued`/`turn_degraded`.
 *
 * Custody is **not** resolution. The lead runner has durably accepted the
 * command, so Desktop must not mint a second one — but the turn has not run,
 * and if that command is later dropped or refused for a non-duplicate reason
 * the fence releases and Desktop's one re-arm becomes due. Only a
 * `turn_started` or a lead echo turns custody into permanent resolution.
 */
export type CodingSessionTeamWakeCustody = {
  sourceEventId: string;
  leadTargetKey: string;
  commandId: string;
  /**
   * Whether the custodian is somebody else's command.
   *
   * Recorded on the row rather than re-derived, because the two ledgers that
   * would answer it are bounded independently: `pending` can evict Desktop's
   * attempt while `custodied` still holds the row, and a re-derivation would
   * then call Desktop's own cover the provider's.
   */
  fromProvider: boolean;
};

/**
 * One source whose Desktop publish threw.
 *
 * Durable rather than mount-local so the row keeps saying "Wake delivery
 * failed" after a restart instead of reverting to "Waiting for the provider
 * wake" — a status word that would be false.
 *
 * It is **disclosure, never a fence**: publish eligibility does not read it, so
 * a later mount retries the source normally and a success clears the row. One
 * transient relay error must not lose a wake for the life of a lead
 * generation. Within a single mount the retry is prevented by the mount-local
 * `failedThisMount` skip set instead.
 */
export type CodingSessionTeamWakePublishFailure = {
  sourceEventId: string;
  leadTargetKey: string;
};

/**
 * Record that one command holds this source's operation at queued/degraded.
 *
 * Idempotent per (source, target, command). Custody is durable so that a
 * remount whose relay window no longer returns the receipt still suppresses:
 * forgetting custody would republish, which is the defect this batch fixes.
 */
export function recordCodingSessionTeamWakeCustody(
  state: CodingSessionTeamWakeState,
  custody: CodingSessionTeamWakeCustody,
): CodingSessionTeamWakeState {
  if (
    !isLedgerEventId(custody.sourceEventId) ||
    !isLedgerText(custody.leadTargetKey) ||
    !isLedgerText(custody.commandId) ||
    state.custodied.some(
      (row) =>
        row.sourceEventId === custody.sourceEventId &&
        row.leadTargetKey === custody.leadTargetKey &&
        row.commandId === custody.commandId,
    )
  ) {
    return state;
  }
  return {
    ...state,
    custodied: [
      ...state.custodied,
      // Same conservative default as the reader: an origin nobody recorded is
      // not evidence that the custodian was ours.
      { ...custody, fromProvider: custody.fromProvider !== false },
    ].slice(-MAX_CUSTODIED_WAKES),
  };
}

/**
 * Drop this source's custody rows for one lead target.
 *
 * Called when the custodian's own receipt says the turn will never run — a
 * non-duplicate `turn_dropped`/`turn_refused` with no start — which is exactly
 * when the runner's fence releases and Desktop's single re-arm becomes due.
 */
export function releaseCodingSessionTeamWakeCustody(
  state: CodingSessionTeamWakeState,
  scope: { sourceEventId: string; leadTargetKey: string },
): CodingSessionTeamWakeState {
  const custodied = state.custodied.filter(
    (row) =>
      row.sourceEventId !== scope.sourceEventId ||
      row.leadTargetKey !== scope.leadTargetKey,
  );
  return custodied.length === state.custodied.length
    ? state
    : { ...state, custodied };
}

export function codingSessionTeamWakeCustodyFor(
  state: CodingSessionTeamWakeState,
  sourceEventId: string,
  leadTargetKey: string,
): readonly CodingSessionTeamWakeCustody[] {
  return state.custodied.filter(
    (row) =>
      row.sourceEventId === sourceEventId &&
      row.leadTargetKey === leadTargetKey,
  );
}

export function recordCodingSessionTeamWakePublishFailure(
  state: CodingSessionTeamWakeState,
  scope: CodingSessionTeamWakePublishFailure,
): CodingSessionTeamWakeState {
  if (
    !isLedgerEventId(scope.sourceEventId) ||
    !isLedgerText(scope.leadTargetKey) ||
    codingSessionTeamWakePublishFailed(
      state,
      scope.sourceEventId,
      scope.leadTargetKey,
    )
  ) {
    return state;
  }
  return {
    ...state,
    publishFailed: [...state.publishFailed, { ...scope }].slice(
      -MAX_PUBLISH_FAILURES,
    ),
  };
}

export function clearCodingSessionTeamWakePublishFailure(
  state: CodingSessionTeamWakeState,
  scope: CodingSessionTeamWakePublishFailure,
): CodingSessionTeamWakeState {
  const publishFailed = state.publishFailed.filter(
    (row) =>
      row.sourceEventId !== scope.sourceEventId ||
      row.leadTargetKey !== scope.leadTargetKey,
  );
  return publishFailed.length === state.publishFailed.length
    ? state
    : { ...state, publishFailed };
}

export function codingSessionTeamWakePublishFailed(
  state: CodingSessionTeamWakeState,
  sourceEventId: string,
  leadTargetKey: string,
): boolean {
  return state.publishFailed.some(
    (row) =>
      row.sourceEventId === sourceEventId &&
      row.leadTargetKey === leadTargetKey,
  );
}

/**
 * Record the one Desktop re-arm this source and target will ever get.
 *
 * Idempotent by (source, target): a second call returns the state unchanged,
 * so a remount that replays the same decision cannot spend a second turn
 * (I14).
 */
export function recordCodingSessionTeamWakeReArm(
  state: CodingSessionTeamWakeState,
  reArm: CodingSessionTeamWakeReArm,
): CodingSessionTeamWakeState {
  if (
    !isLedgerEventId(reArm.sourceEventId) ||
    !isLedgerText(reArm.leadTargetKey) ||
    !isLedgerText(reArm.commandId) ||
    codingSessionTeamWakeReArmCount(
      state,
      reArm.sourceEventId,
      reArm.leadTargetKey,
    ) === 1
  ) {
    return state;
  }
  return {
    ...state,
    reArmed: [...state.reArmed, { ...reArm }].slice(-MAX_RE_ARMED_WAKES),
  };
}

export function codingSessionTeamWakeReArmCount(
  state: CodingSessionTeamWakeState,
  sourceEventId: string,
  leadTargetKey: string,
): 0 | 1 {
  return state.reArmed.some(
    (row) =>
      row.sourceEventId === sourceEventId &&
      row.leadTargetKey === leadTargetKey,
  )
    ? 1
    : 0;
}
