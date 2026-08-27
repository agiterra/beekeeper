/**
 * An agent seat's channel membership, as an observed fact.
 *
 * The relay accepts a 44220 only from a channel member, so an agent seated on
 * an execution that is not in the session channel is a seat that can never
 * speak — and, worse, a seat whose silence looks like an idle agent rather
 * than a refused write. So the actor joins the channel *before* its create is
 * published, and the join is only real once the roster shows it.
 *
 * This mirrors `providerChannelMembership.ts` deliberately: same relay
 * behaviour (an `accepted: true` for kind 9000 means "stored", not
 * "applied"), same read-back, different copy — a person who cannot add an
 * agent to a channel needs to be told that, not told about a provider.
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
/** Pause between roster read-backs. */
const READ_BACK_DELAY_MS = 500;

export type EnsureActorChannelMembershipDeps = {
  addChannelMembers: (input: {
    channelId: string;
    pubkeys: string[];
    role: string;
  }) => Promise<AddChannelMembersResult>;
  getChannelMembers: (channelId: string) => Promise<ChannelMember[]>;
  delayMs?: number;
  attempts?: number;
};

/**
 * Add the seat's actor to the session channel and verify the roster shows it.
 *
 * Throws with actionable copy when the add errored (anything but an "already
 * a member" echo) or when the roster never reflects it. The caller must let
 * that throw abort the create — publishing anyway mints a seat the relay will
 * refuse every turn from.
 */
export async function ensureActorChannelMembership(input: {
  channelId: string;
  actorPubkey: string;
  actorLabel?: string | null;
  channelLabel?: string | null;
  deps?: Partial<EnsureActorChannelMembershipDeps>;
}): Promise<void> {
  const addMembers = input.deps?.addChannelMembers ?? defaultAddChannelMembers;
  const getMembers = input.deps?.getChannelMembers ?? defaultGetChannelMembers;
  const attempts = input.deps?.attempts ?? READ_BACK_ATTEMPTS;
  const delayMs = input.deps?.delayMs ?? READ_BACK_DELAY_MS;
  const actorPubkey = normalizePubkey(input.actorPubkey);
  const channel = input.channelLabel
    ? `#${input.channelLabel}`
    : "this channel";
  const who = input.actorLabel?.trim() || "the agent";

  const result = await addMembers({
    channelId: input.channelId,
    pubkeys: [input.actorPubkey],
    role: "bot",
  });
  const failure = result.errors.find(
    ({ error }) => !error.toLowerCase().includes("already"),
  );
  if (failure) {
    throw new Error(
      `Could not add ${who} to ${channel}: ${failure.error} ` +
        "The session was not created. Ask the channel's owner to add the " +
        "agent, or seat it in a channel you own.",
    );
  }

  for (let attempt = 0; attempt < attempts; attempt += 1) {
    if (attempt > 0) {
      await new Promise((resolve) => setTimeout(resolve, delayMs));
    }
    const members = await getMembers(input.channelId).catch(() => []);
    if (
      members.some((member) => normalizePubkey(member.pubkey) === actorPubkey)
    ) {
      return;
    }
  }
  throw new Error(
    `${who} was reported added to ${channel} but never appeared in its ` +
      "member list, so a seat there could not send anything. The session " +
      "was not created — try again, or ask the channel's owner to add the " +
      "agent.",
  );
}
