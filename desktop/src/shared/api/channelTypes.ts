// Channel domain types — split out of types.ts (which re-exports them) to
// keep that file under the repo's file-size guard.

export type ChannelType = "stream" | "forum" | "dm" | "transport";
export type ChannelVisibility = "open" | "private";
export type ChannelRole = "owner" | "admin" | "member" | "guest" | "bot";

/**
 * Whether a channel is a hidden per-project session transport.
 *
 * Transport channels carry machine traffic (coding-session events) and are
 * never a user surface: `useChannelsQuery` filters them from its returned
 * list by default, so every downstream channel list (sidebar, pickers,
 * search, mentions, member views) hides them without per-surface checks.
 * Identity is the relay-assigned `channel_type` — never the display name,
 * which a person could reuse for a real channel.
 */
export function isSessionTransportChannel(
  channel: Pick<Channel, "channelType">,
): boolean {
  return channel.channelType === "transport";
}

export type Channel = {
  id: string;
  name: string;
  channelType: ChannelType;
  visibility: ChannelVisibility;
  description: string;
  topic: string | null;
  purpose: string | null;
  memberCount: number;
  memberPubkeys: string[];
  lastMessageAt: string | null;
  archivedAt: string | null;
  participants: string[];
  participantPubkeys: string[];
  isMember: boolean;
  ttlSeconds: number | null;
  ttlDeadline: string | null;
  /** Project container coordinate (`30621:<owner>:<slug>`) this channel
   * back-references, when it was created inside a project. */
  projectRef?: string | null;
};

export type ChannelDetail = Channel & {
  createdBy: string;
  createdAt: string;
  updatedAt: string;
  topicSetBy: string | null;
  topicSetAt: string | null;
  purposeSetBy: string | null;
  purposeSetAt: string | null;
  topicRequired: boolean;
  maxMembers: number | null;
  nip29GroupId: string | null;
};

export type ChannelMember = {
  pubkey: string;
  role: ChannelRole;
  isAgent: boolean;
  joinedAt: string;
  displayName: string | null;
};

export type CreateChannelInput = {
  name: string;
  channelType: Exclude<ChannelType, "dm">;
  visibility: ChannelVisibility;
  description?: string;
  ttlSeconds?: number;
  /** Project container coordinate (`30621:<owner>:<slug>`) to create the
   * channel inside — becomes the `project` tag on the kind:9007 event. */
  projectRef?: string | null;
};

export type OpenDmInput = {
  pubkeys: string[];
};

export type UpdateChannelInput = {
  channelId: string;
  name?: string;
  description?: string;
  visibility?: ChannelVisibility;
  /** Omit to leave unchanged, `null` to clear (permanent), or a positive number of seconds to set. */
  ttlSeconds?: number | null;
  /** Project container coordinate (`30621:<owner>:<slug>`).
   * Omit = unchanged, `null` = clear, coordinate = move into that project. */
  project?: string | null;
};

export type SetChannelTopicInput = {
  channelId: string;
  topic: string;
};

export type SetChannelPurposeInput = {
  channelId: string;
  purpose: string;
};

export type CanvasResponse = {
  content: string | null;
  updatedAt: number | null;
  author: string | null;
};

export type SetCanvasInput = {
  channelId: string;
  content: string;
};

export type SetCanvasResult = {
  ok: boolean;
  eventId: string;
};

export type AddChannelMembersInput = {
  channelId: string;
  pubkeys: string[];
  role?: Exclude<ChannelRole, "owner">;
};

export type AddChannelMembersResult = {
  added: string[];
  errors: Array<{
    pubkey: string;
    error: string;
  }>;
};
