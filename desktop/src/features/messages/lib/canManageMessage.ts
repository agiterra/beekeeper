import { KIND_HUDDLE_STARTED } from "@/shared/constants/kinds";
import { normalizePubkey } from "@/shared/lib/pubkey";
import type { TimelineMessage } from "@/features/messages/types";
import type { UserProfileLookup } from "@/features/profile/lib/identity";
import { ownsAuthorAgent } from "@/features/profile/lib/identity";

/**
 * Which authority lets the current user act on a message.
 *
 * `"self"` covers both self-authored messages and messages written by an agent
 * the current user owns — the relay treats those identically (kind:5 is
 * accepted from the author and from the NIP-OA agent owner). `"moderator"` is
 * a strictly weaker grant: the relay accepts a moderator's kind:9005 delete of
 * someone else's message, but never a kind:5 or an edit of it.
 */
export type MessageManageAuthority = "self" | "moderator";

export type MessageManageOptions = {
  /**
   * True when the current user is a community owner/admin *and* the message
   * lives somewhere a moderator delete is meaningful (a channel, not a DM).
   * Callers resolve this — see `useMessageModeration` in
   * `features/messages/hooks.ts` — so this module stays a pure function.
   */
  isModerator?: boolean;
};

/**
 * Returns the authority under which the current user may act on `message`, or
 * `null` when they may not act at all.
 *
 * This deliberately mirrors the relay's authorization for the *delete* paths
 * (`validate_standard_deletion_event` for kind:5 and the kind:9005 arm of
 * `validate_admin_event` in `buzz-relay/src/handlers/side_effects.rs`):
 *
 *   1. `"self"` — the current user pubkey matches the message pubkey, or the
 *      message author's profile carries an `ownerPubkey` (NIP-OA owner record)
 *      equal to the current user's pubkey. Both are accepted by the relay for
 *      kind:5 *and* kind:9005.
 *   2. `"moderator"` — the caller passed `isModerator`, meaning they hold an
 *      owner/admin role the relay accepts on kind:9005. This grant covers
 *      deletion only; the relay does not accept a moderator edit (kind:40003)
 *      of another user's message, so callers must not offer editing for it.
 *
 * Huddle-started messages are immutable regardless of authorship.
 */
export function messageManageAuthority(
  message: TimelineMessage,
  currentPubkey: string | undefined,
  profiles: UserProfileLookup | undefined,
  options?: MessageManageOptions,
): MessageManageAuthority | null {
  if (message.kind === KIND_HUDDLE_STARTED) return null;
  if (!message.pubkey) return null;
  if (currentPubkey) {
    if (normalizePubkey(message.pubkey) === normalizePubkey(currentPubkey)) {
      return "self";
    }
    if (
      ownsAuthorAgent(
        profiles?.[normalizePubkey(message.pubkey)],
        currentPubkey,
      )
    ) {
      return "self";
    }
  }
  return options?.isModerator ? "moderator" : null;
}

/**
 * True when deleting a message authored by `authorPubkey` must go out under
 * **moderator** authority (kind:9005) instead of the silent author path
 * (kind:5).
 *
 * The same self-first rule [`messageManageAuthority`] applies, expressed for
 * callers that hold only the author pubkey rather than a `TimelineMessage` —
 * the moderation queue, which resolves the reported event's signer. It matters
 * because kind:9005 is answered with a **public** `message_deleted` tombstone
 * naming the actor: a moderator resolving a report against their own message
 * must not publish a notice in the channel announcing that they deleted
 * themselves.
 *
 * Unknown either side ⇒ `true`: without an identity there is no self claim to
 * make, and the relay is the authority on whether the moderator path is allowed.
 */
export function requiresModeratorDelete(
  authorPubkey: string | null | undefined,
  currentPubkey: string | null | undefined,
): boolean {
  if (!authorPubkey || !currentPubkey) return true;
  return normalizePubkey(authorPubkey) !== normalizePubkey(currentPubkey);
}

/**
 * True when the current user may act on `message` under some authority.
 *
 * Without `options.isModerator` this is the author-only test — the one the
 * edit and send-to-channel affordances need, since neither is granted to
 * moderators. Pass `options.isModerator` only where a moderator-authority
 * action (kind:9005 delete) is the thing being gated.
 */
export function canManageMessageForCurrentUser(
  message: TimelineMessage,
  currentPubkey: string | undefined,
  profiles: UserProfileLookup | undefined,
  options?: MessageManageOptions,
): boolean {
  return (
    messageManageAuthority(message, currentPubkey, profiles, options) !== null
  );
}
