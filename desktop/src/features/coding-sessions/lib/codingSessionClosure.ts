import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_CODING_SESSION_CLOSURE } from "@/shared/constants/kinds";
import { hasValidSignature } from "@/shared/lib/authors";
import {
  hasExactKeys,
  isCodingSessionSessionRef,
  isPlainRecord,
  normalizePubkey,
  parseBoundedJson,
  parseExactTags,
} from "./codingSessionWireDecode";

export const CODING_SESSION_CLOSURE_TAG_VERSION = "cscl1-1" as const;
export const CODING_SESSION_CLOSURE_SCHEMA_VERSION = 1 as const;
export const MAX_CODING_SESSION_CLOSURE_CONTENT_BYTES = 512;

export type CodingSessionClosureAction = "closed" | "open";

export type CodingSessionClosure = {
  action: CodingSessionClosureAction;
  channelId: string;
  createdAt: number;
  eventId: string;
  founderPubkey: string | null;
  genesisRef: string;
  sessionRef: string;
  signerPubkey: string;
};

type AcceptedCodingSessionClosureListener = (event: RelayEvent) => void;

// A publish receipt and the relay's live echo are independent paths. Accepted
// events fan out to every mounted closure projection so a missed or delayed
// echo cannot leave the workspace and sidebar disagreeing.
const acceptedCodingSessionClosureListeners =
  new Set<AcceptedCodingSessionClosureListener>();

export function subscribeToAcceptedCodingSessionClosures(
  listener: AcceptedCodingSessionClosureListener,
): () => void {
  acceptedCodingSessionClosureListeners.add(listener);
  return () => acceptedCodingSessionClosureListeners.delete(listener);
}

export function codingSessionClosureKey(
  channelId: string,
  sessionRef: string,
  genesisRef: string,
): string {
  return `${channelId}\u0000${sessionRef}\u0000${genesisRef}`;
}

export function buildCodingSessionClosureFilter(
  channelIds: readonly string[],
  limit = 1000,
): RelaySubscriptionFilter {
  return {
    kinds: [KIND_CODING_SESSION_CLOSURE],
    "#h": [...new Set(channelIds)],
    limit,
  };
}

export function buildCodingSessionClosureEvent(input: {
  action: CodingSessionClosureAction;
  channelId: string;
  genesisRef: string;
  sessionRef: string;
}) {
  if (!input.channelId.trim()) throw new Error("channelId must not be empty");
  if (!isCodingSessionSessionRef(input.sessionRef)) {
    throw new Error("sessionRef must be a canonical lowercase hyphenated UUID");
  }
  if (!/^[0-9a-f]{64}$/.test(input.genesisRef)) {
    throw new Error("genesisRef must be a canonical lowercase event id");
  }
  if (input.action !== "closed" && input.action !== "open") {
    throw new Error("action must be open or closed");
  }
  const content = JSON.stringify({
    action: input.action,
    genesisRef: input.genesisRef,
    sessionRef: input.sessionRef,
    v: CODING_SESSION_CLOSURE_SCHEMA_VERSION,
  });
  return {
    kind: KIND_CODING_SESSION_CLOSURE,
    content,
    tags: [
      ["h", input.channelId],
      ["d", input.sessionRef],
      ["cscl-v", CODING_SESSION_CLOSURE_TAG_VERSION],
      ["cscl-genesis", input.genesisRef],
    ],
  };
}

export function parseCodingSessionClosure(
  event: RelayEvent,
): CodingSessionClosure | null {
  if (
    event.kind !== KIND_CODING_SESSION_CLOSURE ||
    !Array.isArray(event.tags)
  ) {
    return null;
  }
  const tags = parseExactTags(event.tags, ["h", "d", "cscl-v", "cscl-genesis"]);
  const signerPubkey = normalizePubkey(event.pubkey);
  if (!tags) return null;
  if (
    !tags[0] ||
    !isCodingSessionSessionRef(tags[1]) ||
    tags[2] !== CODING_SESSION_CLOSURE_TAG_VERSION ||
    !/^[0-9a-f]{64}$/.test(tags[3] ?? "") ||
    !signerPubkey ||
    !hasValidSignature(event)
  ) {
    return null;
  }
  const payload = parseBoundedJson(
    event.content,
    MAX_CODING_SESSION_CLOSURE_CONTENT_BYTES,
  );
  if (
    !isPlainRecord(payload) ||
    !hasExactKeys(payload, ["action", "genesisRef", "sessionRef", "v"]) ||
    (payload.action !== "closed" && payload.action !== "open") ||
    payload.genesisRef !== tags[3] ||
    payload.sessionRef !== tags[1] ||
    payload.v !== CODING_SESSION_CLOSURE_SCHEMA_VERSION
  ) {
    return null;
  }
  return {
    action: payload.action,
    channelId: tags[0],
    createdAt: event.created_at,
    eventId: event.id,
    founderPubkey: null,
    genesisRef: tags[3],
    sessionRef: tags[1],
    signerPubkey,
  };
}

/**
 * Fold the latest relay-accepted, authorized revision for each session.
 *
 * A close is an owner-authority act and only survives when its signer matches
 * the founder reached through the event's explicit genesis id. Reopening is a
 * project-member act; once the explicit genesis resolves, relay acceptance
 * supplies that membership decision for correctly signed `open` events.
 */
export function foldAuthorizedCodingSessionClosures(
  events: readonly RelayEvent[],
  founderPubkeysByGenesisRef: ReadonlyMap<string, string>,
): Map<string, CodingSessionClosure> {
  const latest = new Map<string, CodingSessionClosure>();
  for (const event of events) {
    const closure = parseCodingSessionClosure(event);
    if (!closure) continue;
    const founderPubkey = normalizePubkey(
      founderPubkeysByGenesisRef.get(closure.genesisRef),
    );
    // The explicit genesis id is the only authority link. Until it resolves,
    // neither action is safe to project: even an otherwise valid `open` could
    // be bound to an unrelated or not-yet-observed session origin.
    if (!founderPubkey) continue;
    if (closure.action === "closed" && closure.signerPubkey !== founderPubkey) {
      continue;
    }
    const authorized = { ...closure, founderPubkey };
    const key = codingSessionClosureKey(
      closure.channelId,
      closure.sessionRef,
      closure.genesisRef,
    );
    const previous = latest.get(key);
    if (
      !previous ||
      authorized.createdAt > previous.createdAt ||
      (authorized.createdAt === previous.createdAt &&
        authorized.eventId > previous.eventId)
    ) {
      latest.set(key, authorized);
    }
  }
  return latest;
}

export async function publishCodingSessionClosure(
  input: Parameters<typeof buildCodingSessionClosureEvent>[0],
  dependencies: {
    publisher?: typeof relayClient;
    signer?: typeof signRelayEvent;
  } = {},
): Promise<RelayEvent> {
  const event = await (dependencies.signer ?? signRelayEvent)(
    buildCodingSessionClosureEvent(input),
  );
  const accepted = await (dependencies.publisher ?? relayClient).publishEvent(
    event,
    `Timed out while marking the session ${input.action}.`,
    `Failed to mark the session ${input.action}.`,
  );
  for (const listener of acceptedCodingSessionClosureListeners) {
    try {
      listener(accepted);
    } catch (error) {
      console.error(
        "Failed to apply an accepted coding-session closure",
        error,
      );
    }
  }
  return accepted;
}
