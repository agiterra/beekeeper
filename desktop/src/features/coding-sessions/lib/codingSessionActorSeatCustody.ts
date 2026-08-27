/**
 * Host-local custody of an agent seat's key material, from the webview side.
 *
 * The secret half of a seat's identity never travels on the relay: it is
 * written into the provider's owner-only pending-seat file, keyed by the
 * create's exact `commandId`, and consumed by the provider when it spawns the
 * seat. These two calls are the whole webview surface of that channel — the
 * key itself is read from the OS keyring in Rust and never crosses into
 * JavaScript.
 *
 * Mirrors `desktop/src-tauri/src/managed_agents/actor_seats.rs`.
 */
import { invokeTauri } from "@/shared/api/tauri";

/** What one staging call actually put on disk for the provider to consume. */
export type StagedCodingSessionActorSeat = {
  /**
   * Whether the seat was staged with a role pack. `false` means the seat runs
   * on its prompt alone — no `.agents/skills` will exist in its working
   * directory — and the screen must say so rather than imply craft it lacks.
   */
  packStaged: boolean;
};

/**
 * Stage a managed agent's identity for one exact coding-session create.
 *
 * Call this BEFORE the 44221 is published: the provider refuses a create
 * naming an actor with no staged seat (`ACTOR_UNAVAILABLE`). Rejects when the
 * agent is unknown to this computer or its key is unavailable (a keyring
 * outage), so a create the provider could never honour is never signed.
 *
 * The seat's role pack is resolved on the Rust side from the agent's own
 * provenance — this call names an agent and never a path, so nothing here can
 * choose the directory the provider materializes skills from.
 */
export async function stageCodingSessionActorSeat(input: {
  commandId: string;
  agentPubkey: string;
}): Promise<StagedCodingSessionActorSeat> {
  const staged = await invokeTauri<StagedCodingSessionActorSeat | null>(
    "stage_coding_session_actor_seat",
    {
      commandId: input.commandId,
      agentPubkey: input.agentPubkey,
    },
  );
  return { packStaged: staged?.packStaged === true };
}

/** Drop a staged seat. Succeeds when the provider already consumed it. */
export async function clearCodingSessionActorSeat(
  commandId: string,
): Promise<void> {
  await invokeTauri("clear_coding_session_actor_seat", { commandId });
}
