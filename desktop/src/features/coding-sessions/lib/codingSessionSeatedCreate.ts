/**
 * The order a seated create must happen in, as one testable step.
 *
 * Two things must be true before an actor create is published, and both can
 * fail: the actor has to be a member of the session channel (the relay takes
 * a 44220 from nobody else), and its key material has to be staged
 * host-locally under this exact `commandId` (the provider refuses a create
 * whose actor it cannot resolve, `ACTOR_UNAVAILABLE`). If either fails the
 * create is NOT published and the reason is named — a half-seated session
 * that looks created and answers nothing is the failure this prevents.
 *
 * The publish itself is injected so the guarantee is a property of this
 * function rather than a comment in the create hook.
 */
import type { CodingSessionActorSeat } from "./codingSessionActorSeat";

export type SeatedCodingSessionCreateDeps = {
  /** Add the actor to the channel; throws with actionable copy on failure. */
  ensureMembership: (input: {
    channelId: string;
    actorPubkey: string;
    actorLabel: string | null;
  }) => Promise<void>;
  /** Write the host-local custody entry for this exact command. */
  stageSeat: (input: {
    commandId: string;
    agentPubkey: string;
  }) => Promise<void>;
  /** Drop the custody entry again. Best effort; never fails the create. */
  clearSeat: (commandId: string) => Promise<void>;
};

/**
 * Publish a create, seating an agent first when one was chosen.
 *
 * With no seat this is exactly `publish()` — an unseated create keeps today's
 * behaviour, including doing no channel or custody work at all.
 */
export async function publishSeatedCodingSessionCreate<T>(input: {
  channelId: string;
  commandId: string;
  seat: CodingSessionActorSeat | null;
  seatLabel?: string | null;
  publish: () => Promise<T>;
  deps: SeatedCodingSessionCreateDeps;
}): Promise<T> {
  if (!input.seat) return input.publish();

  await input.deps.ensureMembership({
    channelId: input.channelId,
    actorPubkey: input.seat.actor,
    actorLabel: input.seatLabel ?? null,
  });
  await input.deps.stageSeat({
    commandId: input.commandId,
    agentPubkey: input.seat.actor,
  });
  try {
    return await input.publish();
  } catch (error) {
    // Nothing went out, so the staged secret has no create to belong to.
    await input.deps.clearSeat(input.commandId).catch(() => {});
    throw error;
  }
}
