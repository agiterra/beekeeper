/**
 * Who a mention-send should offer to invite, decided against a membership read
 * that has actually settled (SV-50).
 *
 * The send flow used to treat "channel members not resolved yet" as "nobody to
 * ask about": a mention typed and sent before the member read answered went
 * out with no Invite prompt at all, so the person mentioned never learned they
 * were mentioned in a channel they cannot see. Unknown membership is not
 * membership. This module makes the send wait for the member read, and when
 * that read fails it asks about every mentioned person rather than assuming
 * they are all in the channel — and says the read failed, so the prompt does
 * not claim to know who is outside the channel.
 */
import { type QueryClient, useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import { getChannelMembers } from "@/shared/api/tauriChannels";
import type { ChannelType } from "@/shared/api/types";
import { normalizePubkey } from "@/shared/lib/pubkey";

/** What the send knows about the target channel's members. */
export type MentionMembership =
  | { kind: "resolved"; memberPubkeys: ReadonlySet<string> }
  | { kind: "unknown"; reason: string };

/** Shown on the prompt when the member read failed and every mention is asked about. */
export const MENTION_MEMBERSHIP_UNKNOWN_MESSAGE =
  "This channel's member list could not be read, so some of these people may already be in it.";

/** How long a send waits on an unanswered member read before asking anyway. */
export const MENTION_MEMBERSHIP_READ_TIMEOUT_MS = 10_000;

/**
 * The pubkeys to ask about before sending.
 *
 * DMs and channel-less sends never prompt. With a settled member list the
 * answer is the mentions outside it; with an unknown one it is every mention,
 * because "ask" is the only answer that does not guess.
 */
export function nonMemberMentionPromptPubkeys(input: {
  channelType: ChannelType | null;
  mentionPubkeys: readonly string[];
  membership: MentionMembership;
}): string[] {
  if (input.channelType === null || input.channelType === "dm") return [];
  const unique = [...new Set(input.mentionPubkeys.map(normalizePubkey))];
  const { membership } = input;
  if (membership.kind === "unknown") return unique;
  return unique.filter((pubkey) => !membership.memberPubkeys.has(pubkey));
}

/**
 * The membership a send needs, read only when it can change the answer.
 *
 * DMs, channel-less sends, a send that mentions nobody, and a send whose every
 * mention is exempt from the prompt (a managed or just-created agent) return
 * `null` without reading: no answer the read could give would raise a prompt,
 * so a plain message never waits on the member list.
 */
export function membershipForMentionSend(input: {
  channelType: ChannelType | null;
  mentionPubkeys: readonly string[];
  isExemptFromPrompt: (pubkey: string) => boolean;
  resolve: () => Promise<MentionMembership>;
}): Promise<MentionMembership | null> {
  if (input.channelType === null || input.channelType === "dm") {
    return Promise.resolve(null);
  }
  const needsRead = input.mentionPubkeys.some(
    (pubkey) => !input.isExemptFromPrompt(pubkey),
  );
  return needsRead ? input.resolve() : Promise.resolve(null);
}

/**
 * Settle the target channel's membership.
 *
 * The composer's already-resolved list is used when it belongs to the channel
 * the send targets (or the send names none); a send that prepared a fresh
 * channel reads that channel instead. A failed or channel-less read is `unknown`, never an
 * empty member list and never "everyone is a member".
 */
export async function resolveMentionMembership(input: {
  targetChannelId: string | null;
  composer: {
    channelId: string | null;
    hasResolvedMembers: boolean;
    memberPubkeys: ReadonlySet<string>;
  };
  fetchMemberPubkeys: (channelId: string) => Promise<readonly string[]>;
  /** How long the send waits on the read before treating it as unknown. */
  timeoutMs?: number;
}): Promise<MentionMembership> {
  const { targetChannelId, composer } = input;
  if (
    composer.hasResolvedMembers &&
    (!targetChannelId || composer.channelId === targetChannelId)
  ) {
    return { kind: "resolved", memberPubkeys: composer.memberPubkeys };
  }
  if (!targetChannelId) {
    return { kind: "unknown", reason: "no channel to read members from" };
  }
  // A read that never answers must not hold the send forever: past the
  // deadline the membership is unknown, which asks rather than guesses.
  const timeoutMs = input.timeoutMs ?? MENTION_MEMBERSHIP_READ_TIMEOUT_MS;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const deadline = new Promise<never>((_, reject) => {
    timer = setTimeout(
      () => reject(new Error("the member read did not answer in time")),
      timeoutMs,
    );
  });
  try {
    const pubkeys = await Promise.race([
      input.fetchMemberPubkeys(targetChannelId),
      deadline,
    ]);
    return {
      kind: "resolved",
      memberPubkeys: new Set(pubkeys.map(normalizePubkey)),
    };
  } catch (error) {
    return {
      kind: "unknown",
      reason: error instanceof Error ? error.message : String(error),
    };
  } finally {
    clearTimeout(timer);
  }
}

/** Read members through the shared query cache, joining any read already in flight. */
function fetchChannelMemberPubkeys(
  queryClient: QueryClient,
  channelId: string,
): Promise<string[]> {
  return queryClient
    .fetchQuery({
      queryKey: ["channels", channelId, "members"],
      queryFn: () => getChannelMembers(channelId),
      staleTime: 30_000,
    })
    .then((members) => members.map((member) => member.pubkey));
}

/** The send flow's membership resolver, bound to the composer's current read. */
export function useMentionMembershipResolver(composer: {
  channelId: string | null;
  hasResolvedMembers: boolean;
  memberPubkeys: ReadonlySet<string>;
}): (targetChannelId: string | null) => Promise<MentionMembership> {
  const queryClient = useQueryClient();
  const { channelId, hasResolvedMembers, memberPubkeys } = composer;
  return React.useCallback(
    (targetChannelId) =>
      resolveMentionMembership({
        targetChannelId,
        composer: { channelId, hasResolvedMembers, memberPubkeys },
        fetchMemberPubkeys: (id) => fetchChannelMemberPubkeys(queryClient, id),
      }),
    [channelId, hasResolvedMembers, memberPubkeys, queryClient],
  );
}
