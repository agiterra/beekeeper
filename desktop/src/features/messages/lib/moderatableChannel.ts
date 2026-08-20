import type { ChannelType } from "@/shared/api/types";

/**
 * Which channel types a moderator delete (kind:9005) may act in.
 *
 * A `Record<ChannelType, boolean>` rather than a set of allowed values on
 * purpose: adding a member to the `ChannelType` union breaks this build until
 * someone classifies it, so a new channel type can never inherit moderation by
 * default. The relay enforces the same carve-out at the authorization seam
 * (`decide_authority` in `buzz-relay/src/handlers/moderation_authz.rs`) — this
 * table only decides whether the affordance is offered.
 */
const MODERATABLE_CHANNEL_TYPE: Record<ChannelType, boolean> = {
  stream: true,
  forum: true,
  transport: true,
  // A DM is not a moderated space. The relay refuses a kind:9005 here for
  // every role, so offering the control would be a lie about what it enforces.
  dm: false,
};

/**
 * True only for a **known, resolved, non-DM** channel type.
 *
 * Deliberately a positive test rather than `channelType !== "dm"`. Callers
 * legitimately pass `null`/`undefined` while the channel record is still
 * loading, or for a cold-recovered feed item that never carried a type — and
 * `undefined !== "dm"` is `true`, which read an unresolved channel as "safe to
 * moderate". Unknown means no moderator affordance. It also rejects a runtime
 * string outside the union (the relay knows a `workflow` type the desktop
 * union does not), since the value is not narrowed at the IPC boundary.
 */
export function isModeratableChannelType(
  channelType: ChannelType | null | undefined,
): boolean {
  if (channelType == null) return false;
  return MODERATABLE_CHANNEL_TYPE[channelType] === true;
}
