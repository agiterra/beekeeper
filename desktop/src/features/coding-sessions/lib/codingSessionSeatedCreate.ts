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

export type CodingSessionSeatCustody = {
  /** Write the host-local custody entry for this exact command. */
  stageSeat: (input: {
    commandId: string;
    agentPubkey: string;
  }) => Promise<void>;
  /** Drop the custody entry again. Best effort; never fails the publish. */
  clearSeat: (commandId: string) => Promise<void>;
};

export type SeatedCodingSessionCreateDeps = CodingSessionSeatCustody & {
  /** Add the actor to the channel; throws with actionable copy on failure. */
  ensureMembership: (input: {
    channelId: string;
    actorPubkey: string;
    actorLabel: string | null;
  }) => Promise<void>;
};

/**
 * Stage a seat's key material, publish, and take the entry back if nothing
 * went out.
 *
 * Custody is keyed by `commandId`, and the provider consumes the entry on the
 * command that names it — so **every** command that spawns a process for a
 * seat needs its own entry, not just the create. A resume is exactly that: it
 * mints a fresh `commandId` and starts a new adapter process, which needs the
 * same identity the create gave the first one.
 */
async function publishWithStagedSeat<T>(input: {
  commandId: string;
  actorPubkey: string;
  publish: () => Promise<T>;
  deps: CodingSessionSeatCustody;
}): Promise<T> {
  await input.deps.stageSeat({
    commandId: input.commandId,
    agentPubkey: input.actorPubkey,
  });
  try {
    return await input.publish();
  } catch (error) {
    // Nothing went out, so the staged secret has no command to belong to.
    await input.deps.clearSeat(input.commandId).catch(() => {});
    throw error;
  }
}

/**
 * Reconnect a seated execution, staging its identity for the new generation.
 *
 * The create's custody entry was consumed when the first adapter spawned, so
 * a resume that stages nothing is refused by the provider with
 * `ACTOR_UNAVAILABLE` — permanently, on the very host that holds the key.
 * With no actor this is exactly `publish()`.
 */
export async function publishSeatedCodingSessionResume<T>(input: {
  commandId: string;
  actorPubkey: string | null;
  publish: () => Promise<T>;
  deps: CodingSessionSeatCustody;
}): Promise<T> {
  if (!input.actorPubkey) return input.publish();
  return publishWithStagedSeat({
    commandId: input.commandId,
    actorPubkey: input.actorPubkey,
    publish: input.publish,
    deps: input.deps,
  });
}

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
  return publishWithStagedSeat({
    commandId: input.commandId,
    actorPubkey: input.seat.actor,
    publish: input.publish,
    deps: input.deps,
  });
}
