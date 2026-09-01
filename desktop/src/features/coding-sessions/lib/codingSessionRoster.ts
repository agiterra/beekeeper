import { useCallback } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { getRelaySelf } from "@/features/moderation/lib/relaySelf";
import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_AUTHORITY_TRANSITION,
  KIND_SYSTEM_MESSAGE,
} from "@/shared/constants/kinds";
import {
  hasDuplicateJsonKeys,
  hasExactFields,
} from "@/shared/coordination/sessionCoordinationStrictJson";
import { hasValidSignature } from "@/shared/lib/authors";
import type { EntityRole } from "@/shared/lib/entityRoles";
import { parseExactTags } from "./codingSessionWireDecode";

/**
 * Coding-session roster: the client-side fold of one session's NIP-CSAT
 * authority chain (kind:44228 transitions + their relay-signed kind:40099
 * acceptance receipts) into a display roster, plus the builders and
 * mutations that extend the chain.
 *
 * The relay is the authority — it validates chain linkage (prevAccepted/seq
 * against the current accepted head) and owner signing at ingest, and mints
 * one receipt per accepted transition. This raw-history fold re-verifies each
 * transition signature and only trusts a receipt signed by the active relay's
 * NIP-11 key in the expected channel. It remains advisory display state: a
 * transition without a matching receipt is *pending*, never a grant. The fold
 * fails closed exactly like the session provider's verifier — an unknown
 * transition type, malformed envelope, or receipt whose bound facts disagree
 * with its transition stops the fold at that link. Additive seat links are
 * verified and advance the accepted head, but stay out of this legacy
 * operator/viewer display projection.
 */

const HEX64_REGEX = /^[0-9a-f]{64}$/;
const ROLE_SLUG_REGEX = /^[a-z0-9-]{1,64}$/;
const MAX_U32 = 0xffff_ffff;
const MAX_AUTHORITY_TRANSITION_CONTENT_BYTES = 512;
const textEncoder = new TextEncoder();

/** Version tag pinned on every 44228 transition (`csat-v`). */
export const CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION =
  "csat1-1" as const;

/** `content.type` of the relay-signed kind:40099 acceptance receipt. */
export const CODING_SESSION_AUTHORITY_RECEIPT_TYPE =
  "coding_session_authority_transition_accepted" as const;

/** Legacy transition vocabulary this display surface is allowed to emit. */
export type CodingSessionAuthorityTransitionType =
  | "grant-operator"
  | "grant-viewer"
  | "revoke"
  | "grant-seat"
  | "revoke-seat";

type AcceptedCodingSessionAuthorityTransitionType =
  CodingSessionAuthorityTransitionType;

const ACCEPTED_TRANSITION_TYPES =
  new Set<AcceptedCodingSessionAuthorityTransitionType>([
    "grant-operator",
    "grant-viewer",
    "revoke",
    "grant-seat",
    "revoke-seat",
  ]);

const LEGACY_TRANSITION_FIELDS = [
  "genesisRef",
  "prevAccepted",
  "seq",
  "type",
  "granteePubkey",
] as const;
const SEAT_TRANSITION_FIELDS = [...LEGACY_TRANSITION_FIELDS, "role"] as const;
const LEGACY_RECEIPT_FIELDS = [
  "type",
  "genesisRef",
  "acceptedEventId",
  "seq",
  "transitionType",
  "granteePubkey",
] as const;
const SEAT_RECEIPT_FIELDS = [...LEGACY_RECEIPT_FIELDS, "role"] as const;

/** Wire-grain role a live grant confers (`coding_session_authority_acl`). */
export type CodingSessionRosterRole = "operator" | "viewer";

export type CodingSessionRosterFold = {
  /** Live grants after folding every accepted link in seq order. */
  accepted: Map<string, CodingSessionRosterRole>;
  /** Receipt-backed governed seats, separate from People access roles. */
  activeSeats: Map<string, string>;
  /** The chain's accepted head — what the next transition must extend. */
  acceptedHead: { eventId: string; seq: number } | null;
  /**
   * Grants newer than the accepted head with no receipt yet — best-effort
   * "Inviting…" display only, never authority. Pending revokes carry no
   * displayable role and are omitted.
   */
  pending: { pubkey: string; role: CodingSessionRosterRole; eventId: string }[];
};

/** A display row: shared role vocabulary, founder pinned first as owner. */
export type CodingSessionRosterEntry = {
  pubkey: string;
  role: EntityRole;
  pending?: boolean;
};

type ParsedTransition = {
  eventId: string;
  channelId: string;
  seq: number;
  prevAccepted: string | null;
  type: AcceptedCodingSessionAuthorityTransitionType;
  granteePubkey: string;
  role: string | null;
};

type ParsedReceipt = {
  channelId: string;
  seq: number;
  transitionType: AcceptedCodingSessionAuthorityTransitionType;
  granteePubkey: string;
  role: string | null;
};

/** UI vocabulary for a wire grant role: operator ⇒ collaborator. */
export function entityRoleForRosterRole(
  role: CodingSessionRosterRole,
): EntityRole {
  return role === "operator" ? "collaborator" : "viewer";
}

/** Wire transition type for a grantable UI role. `owner` is not grantable. */
export function grantTypeForEntityRole(
  role: EntityRole,
): Extract<
  CodingSessionAuthorityTransitionType,
  "grant-operator" | "grant-viewer"
> {
  if (role === "collaborator") return "grant-operator";
  if (role === "viewer") return "grant-viewer";
  throw new Error("Sessions have exactly one owner — owner is not grantable.");
}

function rosterRoleForGrantType(type: string): CodingSessionRosterRole | null {
  if (type === "grant-operator") return "operator";
  if (type === "grant-viewer") return "viewer";
  return null;
}

function isAcceptedTransitionType(
  value: unknown,
): value is AcceptedCodingSessionAuthorityTransitionType {
  return (
    typeof value === "string" &&
    ACCEPTED_TRANSITION_TYPES.has(
      value as AcceptedCodingSessionAuthorityTransitionType,
    )
  );
}

function isSeatTransitionType(
  value: AcceptedCodingSessionAuthorityTransitionType,
): value is "grant-seat" | "revoke-seat" {
  return value === "grant-seat" || value === "revoke-seat";
}

function isU32Sequence(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isInteger(value) &&
    value >= 1 &&
    value <= MAX_U32
  );
}

function parseJsonObject(content: string, maxBytes?: number) {
  if (
    (maxBytes !== undefined && textEncoder.encode(content).length > maxBytes) ||
    hasDuplicateJsonKeys(content)
  ) {
    return null;
  }
  try {
    const value: unknown = JSON.parse(content);
    return typeof value === "object" && value !== null && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

function parseTransition(
  event: RelayEvent,
  genesisRef: string,
  expectedChannel: string,
): ParsedTransition | null {
  if (
    event.kind !== KIND_CODING_SESSION_AUTHORITY_TRANSITION ||
    !HEX64_REGEX.test(event.id) ||
    !hasValidSignature(event)
  ) {
    return null;
  }
  const tagValues = parseExactTags(event.tags, ["h", "csat-v", "csat-genesis"]);
  if (
    !tagValues ||
    tagValues[0] !== expectedChannel ||
    tagValues[1] !== CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION ||
    tagValues[2] !== genesisRef
  ) {
    return null;
  }
  const payload = parseJsonObject(
    event.content,
    MAX_AUTHORITY_TRANSITION_CONTENT_BYTES,
  );
  if (!payload || !isAcceptedTransitionType(payload.type)) return null;
  const seatTransition = isSeatTransitionType(payload.type);
  if (
    !hasExactFields(payload, [
      seatTransition ? SEAT_TRANSITION_FIELDS : LEGACY_TRANSITION_FIELDS,
    ])
  ) {
    return null;
  }
  if (payload.genesisRef !== genesisRef) return null;
  const seq = payload.seq;
  if (!isU32Sequence(seq)) return null;
  const prevAccepted = payload.prevAccepted;
  if (
    (prevAccepted !== null &&
      (typeof prevAccepted !== "string" || !HEX64_REGEX.test(prevAccepted))) ||
    (seq === 1) !== (prevAccepted === null) ||
    typeof payload.granteePubkey !== "string" ||
    !HEX64_REGEX.test(payload.granteePubkey) ||
    (seatTransition &&
      (typeof payload.role !== "string" || !ROLE_SLUG_REGEX.test(payload.role)))
  ) {
    return null;
  }
  return {
    eventId: event.id,
    channelId: tagValues[0],
    seq,
    prevAccepted: prevAccepted as string | null,
    type: payload.type,
    granteePubkey: payload.granteePubkey,
    role: seatTransition ? (payload.role as string) : null,
  };
}

function parseReceipt(
  event: RelayEvent,
  genesisRef: string,
  expectedChannel: string,
  trustedRelayPubkey: string,
): { acceptedEventId: string; receipt: ParsedReceipt } | null {
  if (
    event.kind !== KIND_SYSTEM_MESSAGE ||
    event.pubkey !== trustedRelayPubkey ||
    !hasValidSignature(event)
  ) {
    return null;
  }
  const tagValues = parseExactTags(event.tags, ["h"]);
  if (!tagValues || tagValues[0] !== expectedChannel) return null;
  const payload = parseJsonObject(event.content);
  if (!payload || !isAcceptedTransitionType(payload.transitionType)) {
    return null;
  }
  const seatTransition = isSeatTransitionType(payload.transitionType);
  if (
    !hasExactFields(payload, [
      seatTransition ? SEAT_RECEIPT_FIELDS : LEGACY_RECEIPT_FIELDS,
    ])
  ) {
    return null;
  }
  if (payload.type !== CODING_SESSION_AUTHORITY_RECEIPT_TYPE) return null;
  if (payload.genesisRef !== genesisRef) return null;
  if (
    typeof payload.acceptedEventId !== "string" ||
    !HEX64_REGEX.test(payload.acceptedEventId) ||
    !isU32Sequence(payload.seq) ||
    typeof payload.granteePubkey !== "string" ||
    !HEX64_REGEX.test(payload.granteePubkey) ||
    (seatTransition &&
      (typeof payload.role !== "string" || !ROLE_SLUG_REGEX.test(payload.role)))
  ) {
    return null;
  }
  return {
    acceptedEventId: payload.acceptedEventId,
    receipt: {
      channelId: tagValues[0],
      seq: payload.seq,
      transitionType: payload.transitionType,
      granteePubkey: payload.granteePubkey,
      role: seatTransition ? (payload.role as string) : null,
    },
  };
}

/**
 * Fold one session's transitions + receipts into the display roster.
 *
 * Accepted = transitions with a matching relay receipt (by
 * `acceptedEventId`), applied in seq order from 1 while the chain stays
 * contiguous and every link verifies: event id/signature, expected channel,
 * known exact type/shape, a trusted-relay receipt whose facts (seq, type,
 * grantee, and seat role) bind to the transition, unique receipt/canonical
 * sequence, and `prevAccepted` linkage to the previous applied link. Any
 * failed check stops the fold *before* that link
 * — later links are never applied (fail closed, matching the session
 * provider's verifier). Accepted seat links advance the head but do not
 * enter the legacy roster map. Pending = transitions past the accepted head
 * with no receipt yet.
 */
export function foldCodingSessionRoster(input: {
  expectedChannel: string;
  trustedRelayPubkey: string;
  genesisRef: string;
  transitions: readonly RelayEvent[];
  receipts: readonly RelayEvent[];
}): CodingSessionRosterFold {
  const receiptsByAcceptedId = new Map<string, ParsedReceipt>();
  const duplicateReceiptIds = new Set<string>();
  for (const event of input.receipts) {
    const parsed = parseReceipt(
      event,
      input.genesisRef,
      input.expectedChannel,
      input.trustedRelayPubkey,
    );
    if (!parsed) continue;
    if (receiptsByAcceptedId.has(parsed.acceptedEventId)) {
      duplicateReceiptIds.add(parsed.acceptedEventId);
      continue;
    }
    receiptsByAcceptedId.set(parsed.acceptedEventId, parsed.receipt);
  }
  for (const eventId of duplicateReceiptIds)
    receiptsByAcceptedId.delete(eventId);

  const parsedTransitions: ParsedTransition[] = [];
  for (const event of input.transitions) {
    const parsed = parseTransition(
      event,
      input.genesisRef,
      input.expectedChannel,
    );
    if (parsed) parsedTransitions.push(parsed);
  }

  const acceptedBySeq = new Map<number, ParsedTransition[]>();
  for (const transition of parsedTransitions) {
    if (receiptsByAcceptedId.has(transition.eventId)) {
      const candidates = acceptedBySeq.get(transition.seq) ?? [];
      candidates.push(transition);
      acceptedBySeq.set(transition.seq, candidates);
    }
  }

  const accepted = new Map<string, CodingSessionRosterRole>();
  const activeSeats = new Map<string, string>();
  let acceptedHead: { eventId: string; seq: number } | null = null;
  for (let seq = 1; ; seq += 1) {
    const links = acceptedBySeq.get(seq);
    if (links?.length !== 1) break;
    const link = links[0];
    const receipt = receiptsByAcceptedId.get(link.eventId);
    if (
      !receipt ||
      receipt.channelId !== link.channelId ||
      receipt.seq !== link.seq ||
      receipt.transitionType !== link.type ||
      receipt.granteePubkey !== link.granteePubkey ||
      receipt.role !== link.role
    ) {
      break; // Receipt does not bind this link's facts — fail closed.
    }
    if (link.prevAccepted !== (acceptedHead?.eventId ?? null)) {
      break; // Link does not extend the chain we folded — fail closed.
    }
    if (link.type === "revoke") {
      accepted.delete(link.granteePubkey);
    } else if (link.type === "revoke-seat") {
      if (
        link.role === null ||
        activeSeats.get(link.granteePubkey) !== link.role
      ) {
        break;
      }
      activeSeats.delete(link.granteePubkey);
    } else if (link.type === "grant-seat" && link.role !== null) {
      activeSeats.set(link.granteePubkey, link.role);
    } else if (!isSeatTransitionType(link.type)) {
      const role = rosterRoleForGrantType(link.type);
      if (role) accepted.set(link.granteePubkey, role);
    }
    acceptedHead = { eventId: link.eventId, seq };
  }

  const headSeq = acceptedHead?.seq ?? 0;
  const pendingByPubkey = new Map<
    string,
    {
      pubkey: string;
      role: CodingSessionRosterRole;
      eventId: string;
      seq: number;
    }
  >();
  for (const transition of parsedTransitions) {
    if (transition.seq <= headSeq) continue;
    if (receiptsByAcceptedId.has(transition.eventId)) continue;
    const role = rosterRoleForGrantType(transition.type);
    if (!role) continue; // Pending revokes/unknowns are not "Inviting…" rows.
    const existing = pendingByPubkey.get(transition.granteePubkey);
    if (!existing || transition.seq < existing.seq) {
      pendingByPubkey.set(transition.granteePubkey, {
        pubkey: transition.granteePubkey,
        role,
        eventId: transition.eventId,
        seq: transition.seq,
      });
    }
  }
  const pending = [...pendingByPubkey.values()]
    .sort((a, b) => a.seq - b.seq)
    .map(({ pubkey, role, eventId }) => ({ pubkey, role, eventId }));

  return { accepted, activeSeats, acceptedHead, pending };
}

/**
 * The display roster: founder pinned first as the session's one Owner, then
 * live grants in chain order, then pending invites. The founder can never be
 * a grantee, so any stray grant naming the founder is dropped.
 */
export function codingSessionRosterEntries(
  founderPubkey: string | null,
  fold: CodingSessionRosterFold,
): CodingSessionRosterEntry[] {
  const founder = founderPubkey?.toLowerCase() ?? null;
  const entries: CodingSessionRosterEntry[] = founder
    ? [{ pubkey: founder, role: "owner" }]
    : [];
  for (const [pubkey, role] of fold.accepted) {
    if (pubkey === founder) continue;
    entries.push({ pubkey, role: entityRoleForRosterRole(role) });
  }
  const listed = new Set(entries.map((entry) => entry.pubkey));
  for (const invite of fold.pending) {
    if (listed.has(invite.pubkey)) continue;
    listed.add(invite.pubkey);
    entries.push({
      pubkey: invite.pubkey,
      role: entityRoleForRosterRole(invite.role),
      pending: true,
    });
  }
  return entries;
}

export type CodingSessionAuthorityTransitionEventInput = {
  kind: number;
  content: string;
  tags: string[][];
};

/** The exact three-tag public envelope: `h`, `csat-v`, `csat-genesis`. */
export function buildCodingSessionAuthorityTransitionTags(
  channelId: string,
  genesisRef: string,
): string[][] {
  return [
    ["h", channelId],
    ["csat-v", CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION],
    ["csat-genesis", genesisRef],
  ];
}

/** The exact legacy five-field or seat six-field payload the relay decodes. */
export function buildCodingSessionAuthorityTransitionContent(input: {
  genesisRef: string;
  prevAccepted: string | null;
  seq: number;
  type: CodingSessionAuthorityTransitionType;
  granteePubkey: string;
  role?: string;
}): string {
  return JSON.stringify({
    genesisRef: input.genesisRef,
    prevAccepted: input.prevAccepted,
    seq: input.seq,
    type: input.type,
    granteePubkey: input.granteePubkey,
    ...(isSeatTransitionType(input.type) ? { role: input.role } : {}),
  });
}

/**
 * Build one unsigned chain link, validating the same self-consistency the
 * relay's decoder enforces so a doomed submission never leaves the client.
 */
export function buildCodingSessionAuthorityTransitionEvent(input: {
  channelId: string;
  genesisRef: string;
  prevAccepted: string | null;
  seq: number;
  type: CodingSessionAuthorityTransitionType;
  granteePubkey: string;
  role?: string;
}): CodingSessionAuthorityTransitionEventInput {
  if (input.channelId.trim().length === 0) {
    throw new Error("channelId must not be empty");
  }
  if (!HEX64_REGEX.test(input.genesisRef)) {
    throw new Error("genesisRef must be a lowercase 64-hex event id");
  }
  const granteePubkey = input.granteePubkey.toLowerCase();
  if (!HEX64_REGEX.test(granteePubkey)) {
    throw new Error("granteePubkey must be a lowercase 64-hex pubkey");
  }
  if (input.prevAccepted !== null && !HEX64_REGEX.test(input.prevAccepted)) {
    throw new Error("prevAccepted must be null or a lowercase 64-hex event id");
  }
  if (!isU32Sequence(input.seq)) {
    throw new Error("seq must be a u32 integer starting at 1");
  }
  if ((input.seq === 1) !== (input.prevAccepted === null)) {
    throw new Error(
      "seq must be exactly 1 if and only if prevAccepted is null",
    );
  }
  const seatTransition = isSeatTransitionType(input.type);
  if (
    (seatTransition &&
      (typeof input.role !== "string" || !ROLE_SLUG_REGEX.test(input.role))) ||
    (!seatTransition && input.role !== undefined)
  ) {
    throw new Error(
      "role must be one lowercase role slug exactly for a seat transition",
    );
  }
  return {
    kind: KIND_CODING_SESSION_AUTHORITY_TRANSITION,
    content: buildCodingSessionAuthorityTransitionContent({
      genesisRef: input.genesisRef,
      prevAccepted: input.prevAccepted,
      seq: input.seq,
      type: input.type,
      granteePubkey,
      ...(seatTransition ? { role: input.role } : {}),
    }),
    tags: buildCodingSessionAuthorityTransitionTags(
      input.channelId,
      input.genesisRef,
    ),
  };
}

/**
 * Newest-first relay windows cap each fetch; receipts share kind:40099 with
 * ordinary system rows, so the window is sized well past any realistic
 * session-channel volume rather than `limit: 1`.
 */
const ROSTER_EVENT_FETCH_LIMIT = 500;

type RosterEventFetcher = (filter: {
  kinds: number[];
  limit: number;
  "#h": string[];
}) => Promise<RelayEvent[]>;

/**
 * Fetch a channel's transitions + receipts and fold them for one genesis,
 * pinning receipt trust to the active relay's advertised NIP-11 signing key.
 */
export async function fetchCodingSessionRosterFold(
  channelId: string,
  genesisRef: string,
  fetchEvents: RosterEventFetcher = (filter) => relayClient.fetchEvents(filter),
): Promise<CodingSessionRosterFold> {
  const [transitions, receipts, trustedRelayPubkey] = await Promise.all([
    fetchEvents({
      kinds: [KIND_CODING_SESSION_AUTHORITY_TRANSITION],
      "#h": [channelId],
      limit: ROSTER_EVENT_FETCH_LIMIT,
    }),
    fetchEvents({
      kinds: [KIND_SYSTEM_MESSAGE],
      "#h": [channelId],
      limit: ROSTER_EVENT_FETCH_LIMIT,
    }),
    getRelaySelf(),
  ]);
  if (trustedRelayPubkey === null || !HEX64_REGEX.test(trustedRelayPubkey)) {
    throw new Error(
      "The active relay did not advertise a trusted signing key.",
    );
  }
  // Receipts are filtered by `content.type` inside the fold.
  return foldCodingSessionRoster({
    expectedChannel: channelId,
    trustedRelayPubkey,
    genesisRef,
    transitions,
    receipts,
  });
}

export function codingSessionRosterQueryKey(
  channelId: string,
  genesisRef: string | null,
) {
  return ["coding-session-roster", channelId, genesisRef ?? "none"] as const;
}

/**
 * The session roster as display entries, founder pinned first as Owner.
 * Query data is the raw fold (so head resolution stays possible from the
 * cache); `select` maps it to entries.
 */
export function useCodingSessionRoster(
  channelId: string,
  genesisRef: string | null,
  founderPubkey: string | null,
) {
  const select = useCallback(
    (fold: CodingSessionRosterFold) =>
      codingSessionRosterEntries(founderPubkey, fold),
    [founderPubkey],
  );
  return useQuery({
    enabled: channelId.length > 0 && genesisRef !== null,
    queryKey: codingSessionRosterQueryKey(channelId, genesisRef),
    queryFn: () => {
      if (genesisRef === null) throw new Error("No session genesis.");
      return fetchCodingSessionRosterFold(channelId, genesisRef);
    },
    staleTime: 15_000,
    select,
  });
}

/** True for the relay's chain-linkage refusals (stale head / wrong seq). */
export function isCodingSessionAuthorityHeadConflict(error: unknown): boolean {
  return error instanceof Error && /prevAccepted|seq/i.test(error.message);
}

type TransitionPublishDependencies = {
  fetchFold?: (
    channelId: string,
    genesisRef: string,
  ) => Promise<CodingSessionRosterFold>;
  signer?: (
    input: CodingSessionAuthorityTransitionEventInput,
  ) => Promise<RelayEvent>;
  publisher?: {
    publishEvent: (
      event: RelayEvent,
      timeoutMessage: string,
      sendErrorMessage: string,
    ) => Promise<RelayEvent>;
  };
};

/**
 * Extend the chain by one link: resolve the current accepted head fresh from
 * the relay, build seq = head+1 (or the 1/null first link), sign, publish.
 * A relay refusal about the head/seq means someone else extended the chain
 * between our read and write — re-resolve and retry exactly once.
 */
export async function publishCodingSessionAuthorityTransition(
  input: {
    channelId: string;
    genesisRef: string;
    type: CodingSessionAuthorityTransitionType;
    granteePubkey: string;
    role?: string;
  },
  dependencies: TransitionPublishDependencies = {},
): Promise<RelayEvent> {
  const fetchFold = dependencies.fetchFold ?? fetchCodingSessionRosterFold;
  const signer = dependencies.signer ?? signRelayEvent;
  const publisher = dependencies.publisher ?? relayClient;

  const attempt = async () => {
    const fold = await fetchFold(input.channelId, input.genesisRef);
    const head = fold.acceptedHead;
    if (head?.seq === MAX_U32) {
      throw new Error(
        "The coding-session authority chain is exhausted at its u32 maximum.",
      );
    }
    const event = await signer(
      buildCodingSessionAuthorityTransitionEvent({
        channelId: input.channelId,
        genesisRef: input.genesisRef,
        prevAccepted: head?.eventId ?? null,
        seq: (head?.seq ?? 0) + 1,
        type: input.type,
        granteePubkey: input.granteePubkey,
        ...(input.role !== undefined ? { role: input.role } : {}),
      }),
    );
    return publisher.publishEvent(
      event,
      "Timed out updating session access.",
      "Failed to update session access.",
    );
  };

  try {
    return await attempt();
  } catch (error) {
    if (!isCodingSessionAuthorityHeadConflict(error)) throw error;
    return attempt();
  }
}

/** Grant a pubkey collaborator (grant-operator) or viewer (grant-viewer). */
export function useCodingSessionGrantMutation(
  channelId: string,
  genesisRef: string | null,
) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { pubkey: string; role: EntityRole }) => {
      if (genesisRef === null) throw new Error("No session genesis.");
      return publishCodingSessionAuthorityTransition({
        channelId,
        genesisRef,
        type: grantTypeForEntityRole(input.role),
        granteePubkey: input.pubkey,
      });
    },
    onSettled: async () => {
      await queryClient.invalidateQueries({
        queryKey: codingSessionRosterQueryKey(channelId, genesisRef),
      });
    },
  });
}

/** Revoke whatever grant a pubkey currently holds. */
export function useCodingSessionRevokeMutation(
  channelId: string,
  genesisRef: string | null,
) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { pubkey: string }) => {
      if (genesisRef === null) throw new Error("No session genesis.");
      return publishCodingSessionAuthorityTransition({
        channelId,
        genesisRef,
        type: "revoke",
        granteePubkey: input.pubkey,
      });
    },
    onSettled: async () => {
      await queryClient.invalidateQueries({
        queryKey: codingSessionRosterQueryKey(channelId, genesisRef),
      });
    },
  });
}
