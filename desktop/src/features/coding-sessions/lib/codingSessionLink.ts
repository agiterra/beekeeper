/** Session navigation coordinates; unlike a quoted-fact link, no transcript is required. */
import { encodeStructuredKey } from "./codingSessionKeys";

export type CodingSessionLink = {
  channelId: string;
  providerPubkey: string;
  targetKey: string;
};

const PREFIX = "beekeeper://coding-session?";

/** Build a link to an execution using its provider-confirmed target. */
export function buildCodingSessionLink(link: CodingSessionLink): string {
  return `${PREFIX}${new URLSearchParams({ channel: link.channelId, provider: link.providerPubkey, target: link.targetKey })}`;
}

/** Qualify the target with its channel and signer, as the catalog does. */
export function codingSessionLinkGenerationId(link: CodingSessionLink): string {
  return (
    encodeStructuredKey(
      "coding-session-transcript-generation/v1",
      link.channelId,
      link.providerPubkey,
    ) + link.targetKey.slice("coding-session/v1|".length)
  );
}

/** Parse navigation links only. Fact links with `seq` retain their separate contract. */
export function parseCodingSessionLink(
  value: string,
): CodingSessionLink | null {
  if (!value.startsWith(PREFIX) || value.includes("#")) return null;
  const params = new URLSearchParams(value.slice(PREFIX.length));
  if (
    [...params.keys()].length !== 3 ||
    params.getAll("channel").length !== 1 ||
    params.getAll("provider").length !== 1 ||
    params.getAll("target").length !== 1
  )
    return null;
  const channelId = params.get("channel") ?? "";
  const targetKey = params.get("target") ?? "";
  const providerPubkey = params.get("provider") ?? "";
  // Targets are opaque route identifiers. Navigation never establishes
  // authority or execution existence; the destination resolves signed facts.
  if (
    !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(
      channelId,
    ) ||
    !/^[0-9a-f]{64}$/.test(providerPubkey) ||
    !targetKey.startsWith("coding-session/v1|") ||
    targetKey.length <= "coding-session/v1|".length ||
    targetKey.length > 4096 ||
    [...targetKey].some(
      (char) => char.charCodeAt(0) < 32 || char.charCodeAt(0) === 127,
    )
  )
    return null;
  return { channelId, providerPubkey, targetKey };
}
