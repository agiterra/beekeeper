/**
 * Provider membership as an observed fact, not a hopeful publish.
 *
 * The relay's `accepted: true` for a kind 9000 means "stored", not "applied":
 * an add whose side effect fails after storage reports success while the
 * roster never changes, and a 44221 create published into that channel is
 * invisible to the provider forever. So the create flow must not proceed on
 * the publish alone — it inspects the per-pubkey errors and then reads the
 * roster back until the provider actually appears.
 */
import type {
  AddChannelMembersResult,
  ChannelMember,
} from "@/shared/api/types";
import { addChannelMembers as defaultAddChannelMembers } from "@/shared/api/tauri";
import { getChannelMembers as defaultGetChannelMembers } from "@/shared/api/tauriChannels";
import { normalizePubkey } from "./codingSessionWireDecode";

/** Roster read-back attempts before the add is declared not applied. */
const READ_BACK_ATTEMPTS = 3;
/** Pause between roster read-backs — the relay applies adds in-band, so the
 * second attempt usually already sees it; the third absorbs fan-out lag. */
const READ_BACK_DELAY_MS = 500;

export type EnsureProviderChannelMembershipDeps = {
  addChannelMembers: (input: {
    channelId: string;
    pubkeys: string[];
    role: string;
  }) => Promise<AddChannelMembersResult>;
  getChannelMembers: (channelId: string) => Promise<ChannelMember[]>;
  delayMs?: number;
  attempts?: number;
};

function describeChannel(channelLabel: string | null | undefined): string {
  return channelLabel ? `#${channelLabel}` : "this channel";
}

/**
 * Add the provider to the channel and verify the roster reflects it.
 *
 * Throws with actionable copy when the add errored (anything but an
 * "already a member" echo) or when the roster never shows the provider —
 * the observable signature of a membership write the relay stored but never
 * applied.
 */
export async function ensureProviderChannelMembership(input: {
  channelId: string;
  providerPubkey: string;
  channelLabel?: string | null;
  deps?: Partial<EnsureProviderChannelMembershipDeps>;
}): Promise<void> {
  const addMembers = input.deps?.addChannelMembers ?? defaultAddChannelMembers;
  const getMembers = input.deps?.getChannelMembers ?? defaultGetChannelMembers;
  const attempts = input.deps?.attempts ?? READ_BACK_ATTEMPTS;
  const delayMs = input.deps?.delayMs ?? READ_BACK_DELAY_MS;
  const providerPubkey = normalizePubkey(input.providerPubkey);
  const channel = describeChannel(input.channelLabel);

  const result = await addMembers({
    channelId: input.channelId,
    pubkeys: [input.providerPubkey],
    role: "bot",
  });
  const failure = result.errors.find(
    ({ error }) => !error.toLowerCase().includes("already"),
  );
  if (failure) {
    throw new Error(
      `Could not add the session provider to ${channel}: ${failure.error} ` +
        "Ask the channel's owner to add the provider, or start the session " +
        "from a channel you own.",
    );
  }

  for (let attempt = 0; attempt < attempts; attempt += 1) {
    if (attempt > 0) {
      await new Promise((resolve) => setTimeout(resolve, delayMs));
    }
    const members = await getMembers(input.channelId).catch(() => []);
    const present = members.some(
      (member) => normalizePubkey(member.pubkey) === providerPubkey,
    );
    if (present) return;
  }
  throw new Error(
    `The session provider was reported added to ${channel} but never ` +
      "appeared in its member list — the relay may have silently rejected " +
      "the grant. Try again, or ask the channel's owner to add the provider.",
  );
}
