import type {
  ChannelMember,
  ChannelRole,
  ChannelType,
} from "@/shared/api/types";

import { isModeratableChannelType } from "./moderatableChannel";

/**
 * Which `channel_members` roles carry channel-local moderation authority.
 *
 * A `Record<ChannelRole, boolean>` rather than a set of allowed values, for the
 * same reason as `MODERATABLE_CHANNEL_TYPE`: adding a member to the
 * `ChannelRole` union breaks this build until someone classifies it, so a new
 * role can never inherit moderation by default.
 *
 * Mirrors the relay's `ChannelRole` arm in `decide_authority`
 * (`crates/buzz-relay/src/handlers/moderation_authz.rs`), which grants
 * DeleteMessage/Kick to `Some("owner") | Some("admin")` and nothing else.
 */
const CHANNEL_ROLE_MODERATES: Record<ChannelRole, boolean> = {
  owner: true,
  admin: true,
  member: false,
  guest: false,
  bot: false,
};

/**
 * True only for a **known** channel role that the relay would accept as
 * channel-local moderation authority.
 *
 * `null`/`undefined` means the viewer's channel role is unresolved — not a
 * member, or the members query has not produced a confirmed answer. Unknown
 * fails closed. A runtime string outside the union (the value is not narrowed
 * at the IPC boundary) also fails closed.
 */
export function channelRoleModerates(
  role: ChannelRole | null | undefined,
): boolean {
  if (role == null) return false;
  return CHANNEL_ROLE_MODERATES[role] === true;
}

/**
 * The viewer's role in one channel, or `null` when it cannot be confirmed.
 *
 * `members` is `undefined` whenever the members query has not resolved
 * successfully — still loading, disabled, or errored. Never guess a role from
 * an unresolved list: the whole point of this seam is that the affordance
 * matches an authority the relay actually granted.
 */
export function viewerChannelRole(
  members: readonly ChannelMember[] | undefined,
  viewerPubkey: string | null | undefined,
): ChannelRole | null {
  if (!members || !viewerPubkey) return null;
  const target = viewerPubkey.trim().toLowerCase();
  if (target.length === 0) return null;
  const row = members.find(
    (member) => member.pubkey?.trim().toLowerCase() === target,
  );
  return row ? row.role : null;
}

export type MessageModerationInput = {
  /** The channel the message lives in; `null`/`undefined` grants nothing. */
  channelId: string | null | undefined;
  /** Resolved channel type; unknown grants nothing (see `moderatableChannel`). */
  channelType: ChannelType | null | undefined;
  /** Viewer holds community `owner`/`admin` in `relay_members`. */
  isCommunityManager: boolean;
  /** Viewer's pubkey; `undefined` while identity is unresolved. */
  viewerPubkey: string | null | undefined;
  /**
   * The channel's `channel_members` rows — **only** when the members query
   * resolved successfully. Pass `undefined` for pending, disabled, and errored
   * states so an unconfirmed role can never grant the control.
   */
  channelMembers: readonly ChannelMember[] | undefined;
};

/**
 * Whether to offer the kind:9005 moderator delete, mirroring the two
 * authorities the relay grants for `DeleteMessage`.
 *
 * The relay's `decide_authority`
 * (`crates/buzz-relay/src/handlers/moderation_authz.rs`) accepts either a
 * community `owner`/`admin` from `relay_members` (community-wide) or a channel
 * `owner`/`admin` from `channel_members` (within that channel only). The
 * desktop used to implement the first alone, which is why a channel owner with
 * no `relay_members` row saw no delete control on a message the relay would
 * happily have let them delete.
 *
 * Order matters and matches the relay:
 *
 * 1. **DM carve-out first, ahead of every role.** A direct message is not a
 *    moderated space; the relay refuses kind:9005 there for community owner and
 *    channel owner alike, so offering the control would be a lie about what it
 *    enforces. `isModeratableChannelType` is a positive test, so an unresolved
 *    channel type also grants nothing.
 * 2. Community `owner`/`admin` — community-wide, no channel membership needed.
 * 3. Otherwise the viewer's channel role, and only `owner`/`admin`.
 *
 * Every unknown fails closed: no channel id, unresolved channel type,
 * unresolved identity, or an unresolved members query all yield `false`.
 */
export function decideMessageModeration(
  input: MessageModerationInput,
): boolean {
  if (!input.channelId) return false;
  // Ahead of every role — a channel owner of a DM is refused too.
  if (!isModeratableChannelType(input.channelType)) return false;
  if (input.isCommunityManager) return true;
  return channelRoleModerates(
    viewerChannelRole(input.channelMembers, input.viewerPubkey),
  );
}
