/**
 * The small addressable/immutable records that describe an umbrella session
 * rather than an execution: 44226 genesis, 44229 name, 44252 generated title,
 * 44230 closure, 44227 goal.
 *
 * Mirrors the decode halves of desktop's `codingSessionGenesis.ts`,
 * `codingSessionName.ts`, `codingSessionClosure.ts` and `codingSessionGoal.ts`.
 * Bounds are D4's: genesis ≤ 1024 B, name ≤ 256 B on one line, closure
 * ≤ 512 B, goal ≤ 4096 B.
 */
import {
  KIND_CODING_SESSION_CLOSURE,
  KIND_CODING_SESSION_GENERATED_TITLE,
  KIND_CODING_SESSION_GENESIS,
  KIND_CODING_SESSION_GOAL,
  KIND_CODING_SESSION_NAME,
} from "../../../shared/lib/kinds.ts";
import {
  parseCodingSessionTitleParts,
  type SessionNameRecord,
} from "./sessionTitle.ts";
import type { ObservedEvent } from "./types.ts";
import {
  hasExactKeys,
  isCodingSessionSessionRef,
  isPlainRecord,
  normalizePubkey,
  parseBoundedJson,
  parseExactTags,
} from "./wireDecode.ts";

export const CODING_SESSION_GENESIS_TAG_VERSION = "csg1-1" as const;
export const CODING_SESSION_NAME_TAG_VERSION = "csnm1-1" as const;
export const CODING_SESSION_CLOSURE_TAG_VERSION = "cscl1-1" as const;
export const CODING_SESSION_GOAL_TAG_VERSION = "csgl1-1" as const;

export const MAX_CODING_SESSION_GENESIS_CONTENT_BYTES = 1024;
export const MAX_CODING_SESSION_NAME_BYTES = 256;
export const MAX_CODING_SESSION_CLOSURE_CONTENT_BYTES = 512;
export const MAX_CODING_SESSION_GOAL_CONTENT_BYTES = 4096;

export type CodingSessionGenesis = {
  eventId: string;
  channelId: string;
  createdAt: number;
  /** The founder: whoever signed this exact event. */
  founderPubkey: string;
  sessionRef: string;
};

export type CodingSessionName = {
  eventId: string;
  channelId: string;
  createdAt: number;
  signerPubkey: string;
  sessionRef: string;
  name: string;
};

/**
 * One structurally valid 44252. Standing — whether the signer is the provider
 * of the execution its `cs-target` names — is judged later, in the umbrella
 * fold, by {@link resolveSessionDisplayName}; the relay checks shape alone.
 */
export type CodingSessionGeneratedTitle = {
  eventId: string;
  channelId: string;
  createdAt: number;
  signerPubkey: string;
  sessionRef: string;
  targetKey: string;
  title: string;
  model: string;
  /**
   * The 44221 create the title's execution answered. A reader that does not
   * hold that create can fetch it by id to settle the title's standing.
   */
  createEventId: string;
  /** The event as the resolver reads it, without its signature. */
  record: SessionNameRecord;
};

export type CodingSessionClosure = {
  eventId: string;
  channelId: string;
  createdAt: number;
  signerPubkey: string;
  sessionRef: string;
  genesisRef: string;
  action: "closed" | "open";
};

export type CodingSessionGoal = {
  eventId: string;
  channelId: string;
  createdAt: number;
  signerPubkey: string;
  sessionRef: string;
  goal: string;
};

function utf8Bytes(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

/** Decode one 44226, or null. Founder authority is the signer of this event. */
export function parseCodingSessionGenesis(
  event: ObservedEvent,
): CodingSessionGenesis | null {
  if (event.kind !== KIND_CODING_SESSION_GENESIS) return null;
  const tags = parseExactTags(event.tags, ["h", "csg-v", "csg-session"]);
  const founderPubkey = normalizePubkey(event.pubkey);
  if (
    !tags ||
    tags[0].length === 0 ||
    tags[1] !== CODING_SESSION_GENESIS_TAG_VERSION ||
    !isCodingSessionSessionRef(tags[2]) ||
    !founderPubkey
  ) {
    return null;
  }
  const payload = parseBoundedJson(
    event.content,
    MAX_CODING_SESSION_GENESIS_CONTENT_BYTES,
  );
  if (
    !isPlainRecord(payload) ||
    !hasExactKeys(payload, ["sessionRef", "v"]) ||
    payload.sessionRef !== tags[2] ||
    payload.v !== 1
  ) {
    return null;
  }
  return {
    eventId: event.id,
    channelId: tags[0],
    createdAt: event.created_at,
    founderPubkey,
    sessionRef: tags[2],
  };
}

/** Decode one 44229 display name, or null. */
export function parseCodingSessionName(
  event: ObservedEvent,
): CodingSessionName | null {
  if (event.kind !== KIND_CODING_SESSION_NAME) return null;
  const tags = parseExactTags(event.tags, ["h", "d", "csnm-v"]);
  const signerPubkey = normalizePubkey(event.pubkey);
  if (
    !tags ||
    tags[0].length === 0 ||
    !isCodingSessionSessionRef(tags[1]) ||
    tags[2] !== CODING_SESSION_NAME_TAG_VERSION ||
    !signerPubkey ||
    !event.content.trim() ||
    event.content.includes("\n") ||
    event.content.includes("\r") ||
    utf8Bytes(event.content) > MAX_CODING_SESSION_NAME_BYTES
  ) {
    return null;
  }
  return {
    eventId: event.id,
    channelId: tags[0],
    createdAt: event.created_at,
    signerPubkey,
    sessionRef: tags[1],
    name: event.content,
  };
}

/** The resolver's view of a decoded 44229: its exact NIP-CSN envelope. */
export function codingSessionNameRecord(
  name: CodingSessionName,
): SessionNameRecord {
  return {
    id: name.eventId,
    pubkey: name.signerPubkey,
    created_at: name.createdAt,
    kind: KIND_CODING_SESSION_NAME,
    tags: [
      ["h", name.channelId],
      ["d", name.sessionRef],
      ["csnm-v", CODING_SESSION_NAME_TAG_VERSION],
    ],
    content: name.name,
  };
}

/**
 * Decode one 44252 generated title, or null.
 *
 * Shape only, through the same validator the resolver and its shared vectors
 * use, so the store and the fold can never disagree about what is malformed.
 */
export function parseCodingSessionGeneratedTitle(
  event: ObservedEvent,
): CodingSessionGeneratedTitle | null {
  if (event.kind !== KIND_CODING_SESSION_GENERATED_TITLE) return null;
  const signerPubkey = normalizePubkey(event.pubkey);
  if (!signerPubkey || typeof event.content !== "string") return null;
  const envelope = parseCodingSessionTitleParts(event.tags, event.content);
  if (envelope === null) return null;
  return {
    eventId: event.id,
    channelId: envelope.channelId,
    createdAt: event.created_at,
    signerPubkey,
    sessionRef: envelope.sessionRef,
    targetKey: envelope.targetKey,
    title: envelope.payload.title,
    model: envelope.payload.model,
    createEventId: envelope.payload.createEventId,
    record: {
      id: event.id,
      pubkey: signerPubkey,
      created_at: event.created_at,
      kind: event.kind,
      tags: event.tags.map((tag) => [...tag]),
      content: event.content,
    },
  };
}

/** Decode one 44230 closure marker, or null. */
export function parseCodingSessionClosure(
  event: ObservedEvent,
): CodingSessionClosure | null {
  if (event.kind !== KIND_CODING_SESSION_CLOSURE) return null;
  const tags = parseExactTags(event.tags, ["h", "d", "cscl-v", "cscl-genesis"]);
  const signerPubkey = normalizePubkey(event.pubkey);
  if (
    !tags ||
    tags[0].length === 0 ||
    !isCodingSessionSessionRef(tags[1]) ||
    tags[2] !== CODING_SESSION_CLOSURE_TAG_VERSION ||
    !/^[0-9a-f]{64}$/.test(tags[3]) ||
    !signerPubkey
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
    payload.v !== 1
  ) {
    return null;
  }
  return {
    eventId: event.id,
    channelId: tags[0],
    createdAt: event.created_at,
    signerPubkey,
    sessionRef: tags[1],
    genesisRef: tags[3],
    action: payload.action,
  };
}

/** Decode one 44227 goal, or null. */
export function parseCodingSessionGoal(
  event: ObservedEvent,
): CodingSessionGoal | null {
  if (event.kind !== KIND_CODING_SESSION_GOAL) return null;
  const tags = parseExactTags(event.tags, ["h", "d", "csgl-v"]);
  const signerPubkey = normalizePubkey(event.pubkey);
  if (
    !tags ||
    tags[0].length === 0 ||
    !isCodingSessionSessionRef(tags[1]) ||
    tags[2] !== CODING_SESSION_GOAL_TAG_VERSION ||
    !signerPubkey ||
    !event.content.trim() ||
    utf8Bytes(event.content) > MAX_CODING_SESSION_GOAL_CONTENT_BYTES
  ) {
    return null;
  }
  return {
    eventId: event.id,
    channelId: tags[0],
    createdAt: event.created_at,
    signerPubkey,
    sessionRef: tags[1],
    goal: event.content,
  };
}

/**
 * Newest-wins fold with the event-id tie-break, over any of the records above.
 *
 * Second-granularity `created_at` cannot order a burst, so the lexicographic
 * event id breaks the tie deterministically rather than letting arrival order
 * decide.
 */
export function foldNewestByKey<
  T extends { createdAt: number; eventId: string },
>(records: readonly T[], keyOf: (record: T) => string): Map<string, T> {
  const newest = new Map<string, T>();
  for (const record of records) {
    const key = keyOf(record);
    const previous = newest.get(key);
    if (
      !previous ||
      record.createdAt > previous.createdAt ||
      (record.createdAt === previous.createdAt &&
        record.eventId > previous.eventId)
    ) {
      newest.set(key, record);
    }
  }
  return newest;
}
