import type { ChannelType } from "@/shared/api/types";
import { normalizePubkey } from "@/shared/lib/pubkey";
import { messageMentionPubkeys } from "./messageMentionPubkeys";

/** Prepare the same semantic DM recipients publication will address, before agent readiness. */
export async function resolveMessageSendRecipients(input: {
  channelType: ChannelType | null;
  senderPubkey: string | null | undefined;
  explicitMentions: string[];
  resolveMembership: () => Promise<
    | { kind: "resolved"; memberPubkeys: ReadonlySet<string> }
    | { kind: "unknown"; reason: string }
  >;
}): Promise<string[]> {
  // Streams retain explicit mention preparation and never acquire an implicit audience.
  if (input.channelType !== "dm") return input.explicitMentions;
  const sender = normalizePubkey(input.senderPubkey ?? "");
  if (!sender) throw new Error("No identity available for preparing this DM.");
  const membership = await input.resolveMembership();
  if (membership.kind !== "resolved") {
    throw new Error(
      "Could not confirm this DM's members; your message has not been sent.",
    );
  }
  const members = new Set([...membership.memberPubkeys].map(normalizePubkey));
  if (!members.has(sender)) {
    throw new Error(
      "Your membership in this DM could not be confirmed; your message has not been sent.",
    );
  }
  return messageMentionPubkeys(
    { channelType: "dm", memberPubkeys: [...members], participantPubkeys: [] },
    sender,
    input.explicitMentions,
  );
}
