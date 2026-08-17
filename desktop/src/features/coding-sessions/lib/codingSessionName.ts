import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_CODING_SESSION_NAME } from "@/shared/constants/kinds";
import { hasValidSignature } from "@/shared/lib/authors";
import { isCodingSessionSessionRef } from "./codingSessionWireDecode";

export const CODING_SESSION_NAME_TAG_VERSION = "csnm1-1" as const;
export const MAX_CODING_SESSION_NAME_BYTES = 256;

export type CodingSessionName = {
  channelId: string;
  content: string;
  createdAt: number;
  eventId: string;
  founderPubkey: string;
  sessionRef: string;
};

export function codingSessionNameKey(
  channelId: string,
  sessionRef: string,
  founderPubkey: string,
) {
  return `${channelId}\u0000${sessionRef}\u0000${founderPubkey.toLowerCase()}`;
}

export function buildCodingSessionNameFilter(
  channelIds: readonly string[],
  limit = 1000,
): RelaySubscriptionFilter {
  return {
    kinds: [KIND_CODING_SESSION_NAME],
    "#h": [...new Set(channelIds)],
    limit,
  };
}

export function buildCodingSessionNameEvent(input: {
  channelId: string;
  content: string;
  sessionRef: string;
}) {
  const content = input.content.trim();
  if (!input.channelId.trim()) throw new Error("channelId must not be empty");
  if (!isCodingSessionSessionRef(input.sessionRef)) {
    throw new Error("sessionRef must be a canonical lowercase hyphenated UUID");
  }
  const size = new TextEncoder().encode(content).byteLength;
  if (
    size === 0 ||
    size > MAX_CODING_SESSION_NAME_BYTES ||
    content.includes("\n") ||
    content.includes("\r")
  ) {
    throw new Error(
      "Session name must be one line between 1 and 256 UTF-8 bytes.",
    );
  }
  return {
    kind: KIND_CODING_SESSION_NAME,
    content,
    tags: [
      ["h", input.channelId],
      ["d", input.sessionRef],
      ["csnm-v", CODING_SESSION_NAME_TAG_VERSION],
    ],
  };
}

export function parseCodingSessionName(
  event: RelayEvent,
): CodingSessionName | null {
  if (
    event.kind !== KIND_CODING_SESSION_NAME ||
    event.tags.length !== 3 ||
    event.tags[0]?.length !== 2 ||
    event.tags[0]?.[0] !== "h" ||
    !event.tags[0]?.[1] ||
    event.tags[1]?.length !== 2 ||
    event.tags[1]?.[0] !== "d" ||
    !isCodingSessionSessionRef(event.tags[1]?.[1] ?? "") ||
    event.tags[2]?.length !== 2 ||
    event.tags[2]?.[0] !== "csnm-v" ||
    event.tags[2]?.[1] !== CODING_SESSION_NAME_TAG_VERSION ||
    !event.content.trim() ||
    event.content.includes("\n") ||
    event.content.includes("\r") ||
    new TextEncoder().encode(event.content).byteLength >
      MAX_CODING_SESSION_NAME_BYTES ||
    !hasValidSignature(event)
  ) {
    return null;
  }
  return {
    channelId: event.tags[0][1],
    content: event.content,
    createdAt: event.created_at,
    eventId: event.id,
    founderPubkey: event.pubkey.toLowerCase(),
    sessionRef: event.tags[1][1],
  };
}

/** Fold append-only revisions deterministically while retaining source ids. */
export function foldLatestCodingSessionNames(
  events: readonly RelayEvent[],
): Map<string, CodingSessionName> {
  const latest = new Map<string, CodingSessionName>();
  for (const event of events) {
    const name = parseCodingSessionName(event);
    if (!name) continue;
    const key = `${name.channelId}\u0000${name.sessionRef}`;
    const previous = latest.get(key);
    if (
      !previous ||
      name.createdAt > previous.createdAt ||
      (name.createdAt === previous.createdAt && name.eventId > previous.eventId)
    ) {
      latest.set(key, name);
    }
  }
  return latest;
}

/** Preflight-authority view: a foreign signer cannot shadow founder history. */
export function foldLatestCodingSessionNamesByFounder(
  events: readonly RelayEvent[],
): Map<string, CodingSessionName> {
  const latest = new Map<string, CodingSessionName>();
  for (const event of events) {
    const name = parseCodingSessionName(event);
    if (!name) continue;
    const key = `${name.channelId}\u0000${name.sessionRef}\u0000${name.founderPubkey}`;
    const previous = latest.get(key);
    if (
      !previous ||
      name.createdAt > previous.createdAt ||
      (name.createdAt === previous.createdAt && name.eventId > previous.eventId)
    ) {
      latest.set(key, name);
    }
  }
  return latest;
}

export async function publishCodingSessionName(
  input: Parameters<typeof buildCodingSessionNameEvent>[0],
  dependencies: {
    publisher?: typeof relayClient;
    signer?: typeof signRelayEvent;
  } = {},
): Promise<RelayEvent> {
  const event = await (dependencies.signer ?? signRelayEvent)(
    buildCodingSessionNameEvent(input),
  );
  return await (dependencies.publisher ?? relayClient).publishEvent(
    event,
    "Timed out while renaming the session.",
    "Failed to rename the session.",
  );
}
