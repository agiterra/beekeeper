import { invokeTauri } from "@/shared/api/tauri";

/**
 * The person's grant of full access to this computer, per coding session.
 *
 * A granted session's agent is started without the project boundary at its
 * next start — a create, or a restart. The grant lives in this computer's
 * provider state directory, so only a session whose provider is this
 * computer's can be read or changed here.
 *
 * Mirrors `desktop/src-tauri/src/session_provider/full_access.rs`.
 */

/**
 * Whether `sessionId` (the provider's session id — a command target's
 * `sessionId`) has full access on this computer.
 *
 * `null` means the session's provider is not this computer's, so there is
 * nothing to say about it here: show nothing, never "off".
 */
export async function getCodingSessionFullAccess(input: {
  providerPubkey: string;
  sessionId: string;
}): Promise<boolean | null> {
  return invokeTauri<boolean | null>("coding_session_full_access", {
    providerPubkey: input.providerPubkey,
    sessionId: input.sessionId,
  });
}

/**
 * Grant or withdraw full access for `sessionId` on this computer.
 *
 * Takes effect at the session's next start; the caller restarts a running
 * session so the answer is in force at once. Rejects when the session's
 * provider is not this computer's.
 */
export async function setCodingSessionFullAccess(input: {
  providerPubkey: string;
  sessionId: string;
  granted: boolean;
}): Promise<void> {
  await invokeTauri<void>("set_coding_session_full_access", {
    providerPubkey: input.providerPubkey,
    sessionId: input.sessionId,
    granted: input.granted,
  });
}
