/**
 * Granting a hired seat the authority to answer at all.
 *
 * Moved out of `hooks/useCodingSessionHire.ts` unchanged when that file
 * reached the repository's 1000-line ceiling, the same way
 * `codingSessionHireOutcomeStore.ts` was. It is a coherent unit: the receipt
 * gate and the two grants that follow it, and nothing else.
 */
import type { CodingSessionHireDeps } from "../hooks/useCodingSessionHire";
import {
  codingSessionGrantFailureDetail,
  codingSessionGrantFailureReason,
  ensureCodingSessionGrantWithBackoff,
} from "./codingSessionGrantRetry";
import { isCodingSessionCreateRefusal } from "./codingSessionCrewReceipt";
import { disposeRefusedSeatWorktree } from "./codingSessionHireSeatCwd";
import type { CodingSessionHireSeatPlan } from "./codingSessionHireSeat";

/**
 * Confirm the seat, then grant its provider and actor operator authority.
 *
 * Returns null when both landed, or the reason the seat is ungranted. Never
 * throws: a failure here does not un-create the seat, and swallowing it would
 * be the exact silence this exists to end. A create the provider *refused*
 * also removes the worktree cut for it, and the reason says what happened.
 */
export async function grantSeat(
  plan: CodingSessionHireSeatPlan,
  deps: CodingSessionHireDeps,
): Promise<string | null> {
  try {
    await deps.awaitSeatReceipt({
      channelId: plan.channelId,
      commandId: plan.commandId,
      providerAuthorityPubkey: plan.providerAuthorityPubkey,
    });
  } catch (error) {
    const reason = codingSessionGrantFailureReason(error);
    // A refusal receipt proves no execution exists, so the tree cut for it
    // is unused: remove it rather than leave it for the next hire of the same
    // seat to orphan (control run 7's `…-verifier-1`).
    if (!isCodingSessionCreateRefusal(error)) return reason;
    return `${reason} — ${await disposeRefusedSeatWorktree(plan, deps)}`;
  }
  const providerFailure = await ensureCodingSessionGrantWithBackoff({
    grant: () =>
      deps.ensureOperatorGrant({
        channelId: plan.channelId,
        genesisRef: plan.genesisRef,
        granteePubkey: plan.providerAuthorityPubkey,
      }),
    ...(deps.sleep === undefined ? {} : { sleep: deps.sleep }),
    ...(deps.monotonicNow === undefined ? {} : { now: deps.monotonicNow }),
  });
  if (providerFailure !== null) {
    return `provider wake authority: ${codingSessionGrantFailureDetail(providerFailure)}`;
  }
  const actorFailure = await ensureCodingSessionGrantWithBackoff({
    grant: () =>
      deps.ensureOperatorGrant({
        channelId: plan.channelId,
        genesisRef: plan.genesisRef,
        granteePubkey: plan.actor,
      }),
    ...(deps.sleep === undefined ? {} : { sleep: deps.sleep }),
    ...(deps.monotonicNow === undefined ? {} : { now: deps.monotonicNow }),
  });
  if (actorFailure !== null) {
    return `seat actor authority: ${codingSessionGrantFailureDetail(actorFailure)}`;
  }
  return null;
}
