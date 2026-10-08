import type { SessionPreviewAnnounce } from "@/features/coding-sessions/lib/codingSessionSurfaceSnapshot";

/**
 * The Browser's sharing words and routing (SV-33 S3/S4, lane V contract).
 * Pure, so the strings are pinned in one place.
 */

/** The host strip while the Share toggle is off. */
export const SESSION_PREVIEW_NOT_SHARED_LABEL = "Local only · not shared";

/** Said when the session has no sessionRef (VB's `no_session_ref`). */
export const SESSION_PREVIEW_NO_SESSION_REF_SENTENCE =
  "This session has no session reference, so its Browser cannot be shared.";

/** The host honesty strip's text for a share state. */
export function sessionPreviewShareStripText(input: {
  share: boolean;
  watchers: number;
  unavailableSentence: string | null;
}): string {
  if (input.unavailableSentence) return input.unavailableSentence;
  if (!input.share) return SESSION_PREVIEW_NOT_SHARED_LABEL;
  return `Live on this computer · shared with the session · ${input.watchers} watching`;
}

/**
 * Whether the Browser shows the remote view rather than this computer's own
 * preview: the session's agent runs elsewhere (`isLocalProvider === false`),
 * or someone else's open announce owns the session's preview. Needs a
 * sessionRef: without one nothing can be shared, so there is nothing remote
 * to watch.
 */
export function sessionPreviewShowsRemote(input: {
  sessionRef: string | null;
  isLocalProvider: boolean | null;
  owner: Pick<SessionPreviewAnnounce, "signer"> | null;
  currentUserPubkey: string | null;
}): boolean {
  if (!input.sessionRef) return false;
  if (input.isLocalProvider === false) return true;
  const me = input.currentUserPubkey?.trim().toLowerCase() ?? null;
  return input.owner !== null && input.owner.signer !== me;
}
