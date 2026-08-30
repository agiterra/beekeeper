import type { RelayEvent } from "@/shared/api/types";
import { KIND_CODING_SESSION_AUTHORITY_TRANSITION } from "@/shared/constants/kinds";
import { hasValidSignature } from "@/shared/lib/authors";
import {
  hasDuplicateJsonKeys,
  hasExactFields,
} from "@/shared/coordination/sessionCoordinationStrictJson";
import type { StrictDecodeResult } from "./codingSessionTeamTransactionWire";

const KIND_SYSTEM_MESSAGE = 40099;
const CSAT_VERSION = "csat1-1";
const RECEIPT_TYPE = "coding_session_authority_transition_accepted";
const HEX64 = /^[0-9a-f]{64}$/;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const ROLE = /^[a-z0-9-]{1,64}$/;
const encoder = new TextEncoder();

export type CodingSessionAuthorityTransitionType =
  | "grant-operator"
  | "grant-viewer"
  | "revoke"
  | "grant-seat"
  | "revoke-seat";

type AuthorityTransition = {
  genesisRef: string;
  prevAccepted: string | null;
  seq: number;
  type: CodingSessionAuthorityTransitionType;
  granteePubkey: string;
  role?: string;
};

type AuthorityReceipt = {
  type: typeof RECEIPT_TYPE;
  genesisRef: string;
  acceptedEventId: string;
  seq: number;
  transitionType: CodingSessionAuthorityTransitionType;
  granteePubkey: string;
  role?: string;
};

export type CodingSessionMissionAuthorityProjection = {
  channelRef: string;
  genesisRef: string;
  founderPubkey: string;
  relayPubkey: string;
  headEventId: string | null;
  headSeq: number;
  acceptedEventIds: string[];
  activeGrants: Array<{
    actorPubkey: string;
    grantEventRef: string;
    maySteer: boolean;
  }>;
  activeSeats: Array<{
    actorPubkey: string;
    role: string;
    grantEventRef: string;
  }>;
};

function fail<T>(error: string): StrictDecodeResult<T> {
  return { ok: false, error };
}

function parseJson(source: string, maxBytes: number): unknown | null {
  if (
    encoder.encode(source).length > maxBytes ||
    hasDuplicateJsonKeys(source)
  ) {
    return null;
  }
  try {
    return JSON.parse(source);
  } catch {
    return null;
  }
}

function parseCoarseJson(source: string): Record<string, unknown> | null {
  try {
    const value: unknown = JSON.parse(source);
    return typeof value === "object" && value !== null && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

function isSeq(value: unknown): value is number {
  return (
    Number.isInteger(value) &&
    (value as number) >= 1 &&
    (value as number) <= 0xffff_ffff
  );
}

function isTransitionType(
  value: unknown,
): value is CodingSessionAuthorityTransitionType {
  return (
    value === "grant-operator" ||
    value === "grant-viewer" ||
    value === "revoke" ||
    value === "grant-seat" ||
    value === "revoke-seat"
  );
}

function decodeTransitionContent(
  source: string,
): StrictDecodeResult<AuthorityTransition> {
  const value = parseJson(source, 512);
  const isSeat =
    typeof value === "object" &&
    value !== null &&
    ((value as Record<string, unknown>).type === "grant-seat" ||
      (value as Record<string, unknown>).type === "revoke-seat");
  if (
    !hasExactFields(value, [
      isSeat
        ? ["genesisRef", "prevAccepted", "seq", "type", "granteePubkey", "role"]
        : ["genesisRef", "prevAccepted", "seq", "type", "granteePubkey"],
    ]) ||
    typeof value.genesisRef !== "string" ||
    !HEX64.test(value.genesisRef) ||
    !(
      value.prevAccepted === null ||
      (typeof value.prevAccepted === "string" && HEX64.test(value.prevAccepted))
    ) ||
    !isSeq(value.seq) ||
    (value.seq === 1) !== (value.prevAccepted === null) ||
    !isTransitionType(value.type) ||
    typeof value.granteePubkey !== "string" ||
    !HEX64.test(value.granteePubkey) ||
    (isSeat && (typeof value.role !== "string" || !ROLE.test(value.role)))
  ) {
    return fail("authority transition does not match the strict CSAT shape");
  }
  return { ok: true, value: value as AuthorityTransition };
}

function decodeReceiptContent(
  source: string,
): StrictDecodeResult<AuthorityReceipt> {
  const value = parseJson(source, 512);
  const transitionType =
    typeof value === "object" && value !== null
      ? (value as Record<string, unknown>).transitionType
      : null;
  const isSeat =
    transitionType === "grant-seat" || transitionType === "revoke-seat";
  if (
    !hasExactFields(value, [
      isSeat
        ? [
            "type",
            "genesisRef",
            "acceptedEventId",
            "seq",
            "transitionType",
            "granteePubkey",
            "role",
          ]
        : [
            "type",
            "genesisRef",
            "acceptedEventId",
            "seq",
            "transitionType",
            "granteePubkey",
          ],
    ]) ||
    value.type !== RECEIPT_TYPE ||
    typeof value.genesisRef !== "string" ||
    !HEX64.test(value.genesisRef) ||
    typeof value.acceptedEventId !== "string" ||
    !HEX64.test(value.acceptedEventId) ||
    !isSeq(value.seq) ||
    !isTransitionType(value.transitionType) ||
    typeof value.granteePubkey !== "string" ||
    !HEX64.test(value.granteePubkey) ||
    (isSeat && (typeof value.role !== "string" || !ROLE.test(value.role)))
  ) {
    return fail(
      "authority receipt does not match the strict CSAT receipt shape",
    );
  }
  return { ok: true, value: value as AuthorityReceipt };
}

function decodeScopedTransition(input: {
  event: RelayEvent;
  channelRef: string;
  genesisRef: string;
}): StrictDecodeResult<{ signer: string; payload: AuthorityTransition }> {
  if (!hasValidSignature(input.event))
    return fail("authority transition signature is invalid");
  if (input.event.kind !== KIND_CODING_SESSION_AUTHORITY_TRANSITION) {
    return fail("authority transition has the wrong kind");
  }
  const decoded = decodeTransitionContent(input.event.content);
  if (!decoded.ok) return decoded;
  if (
    decoded.value.genesisRef !== input.genesisRef ||
    JSON.stringify(input.event.tags) !==
      JSON.stringify([
        ["h", input.channelRef],
        ["csat-v", CSAT_VERSION],
        ["csat-genesis", input.genesisRef],
      ])
  ) {
    return fail(
      "authority transition crosses or disagrees with its supplied scope",
    );
  }
  return {
    ok: true,
    value: { signer: input.event.pubkey, payload: decoded.value },
  };
}

/**
 * Project authority only from a contiguous chain whose transition signatures
 * and relay-signed acceptance receipts bind every accepted fact exactly.
 */
export function projectCodingSessionMissionAuthority(input: {
  channelRef: string;
  genesisRef: string;
  founderPubkey: string;
  relayPubkey: string;
  transitions: readonly RelayEvent[];
  receipts: readonly RelayEvent[];
}): StrictDecodeResult<CodingSessionMissionAuthorityProjection> {
  if (
    !UUID.test(input.channelRef) ||
    !HEX64.test(input.genesisRef) ||
    !HEX64.test(input.founderPubkey) ||
    !HEX64.test(input.relayPubkey)
  ) {
    return fail("authority scope is not canonical");
  }
  const transitions = new Map<string, RelayEvent>();
  for (const event of input.transitions) {
    if (!transitions.has(event.id)) transitions.set(event.id, event);
  }
  const seenReceiptIds = new Set<string>();
  const seenAcceptedIds = new Set<string>();
  const acceptedBySeq = new Map<
    number,
    { eventId: string; signer: string; payload: AuthorityTransition }
  >();
  for (const event of input.receipts) {
    const coarse = parseCoarseJson(event.content);
    const belongsToChannel = event.tags.some(
      (tag) =>
        tag.length === 2 && tag[0] === "h" && tag[1] === input.channelRef,
    );
    if (
      coarse?.type !== RECEIPT_TYPE ||
      coarse.genesisRef !== input.genesisRef ||
      !belongsToChannel
    ) {
      continue;
    }
    if (seenReceiptIds.has(event.id)) continue;
    seenReceiptIds.add(event.id);
    if (
      event.kind !== KIND_SYSTEM_MESSAGE ||
      event.pubkey !== input.relayPubkey ||
      !hasValidSignature(event) ||
      JSON.stringify(event.tags) !== JSON.stringify([["h", input.channelRef]])
    ) {
      return fail(
        "authority receipt is not a scoped signature from the trusted relay",
      );
    }
    const receipt = decodeReceiptContent(event.content);
    if (!receipt.ok) return receipt;
    const transitionEvent = transitions.get(receipt.value.acceptedEventId);
    if (!transitionEvent)
      return fail("authority receipt references a missing transition");
    const transition = decodeScopedTransition({
      event: transitionEvent,
      channelRef: input.channelRef,
      genesisRef: input.genesisRef,
    });
    if (!transition.ok) return transition;
    const payload = transition.value.payload;
    if (
      receipt.value.genesisRef !== payload.genesisRef ||
      receipt.value.seq !== payload.seq ||
      receipt.value.transitionType !== payload.type ||
      receipt.value.granteePubkey !== payload.granteePubkey ||
      receipt.value.role !== payload.role
    ) {
      return fail(
        "authority receipt facts do not match its accepted transition",
      );
    }
    if (seenAcceptedIds.has(transitionEvent.id)) {
      return fail("duplicate authority receipts name one accepted transition");
    }
    seenAcceptedIds.add(transitionEvent.id);
    if (acceptedBySeq.has(payload.seq)) {
      return fail("conflicting authority receipts claim one sequence");
    }
    acceptedBySeq.set(payload.seq, {
      eventId: transitionEvent.id,
      signer: transition.value.signer,
      payload,
    });
  }

  const grants = new Map<
    string,
    { actorPubkey: string; grantEventRef: string; maySteer: boolean }
  >();
  const seats = new Map<
    string,
    { actorPubkey: string; role: string; grantEventRef: string }
  >();
  let headEventId: string | null = null;
  const acceptedEventIds: string[] = [];
  for (let seq = 1; seq <= acceptedBySeq.size; seq += 1) {
    const link = acceptedBySeq.get(seq);
    if (!link || link.payload.prevAccepted !== headEventId) {
      return fail("accepted authority chain is not contiguous");
    }
    const signerGrant = grants.get(link.signer);
    const signerSeat = seats.get(link.signer);
    const signerIsFounder = link.signer === input.founderPubkey;
    const signerIsOperator = signerGrant?.maySteer === true;
    const signerIsLead = signerSeat?.role === "lead";
    const seatTransition =
      link.payload.type === "grant-seat" || link.payload.type === "revoke-seat";
    if (
      (seatTransition &&
        !signerIsFounder &&
        !signerIsOperator &&
        !signerIsLead) ||
      (!seatTransition && !signerIsFounder)
    ) {
      return fail(
        `authority transition ${link.eventId} has an unauthorized signer`,
      );
    }
    if (
      link.payload.type === "grant-seat" &&
      link.payload.granteePubkey === link.signer
    ) {
      return fail("seat grant cannot nominate its own signer");
    }
    if (
      signerIsLead &&
      !signerIsFounder &&
      !signerIsOperator &&
      link.payload.role === "lead"
    ) {
      return fail("a lead seat cannot grant or revoke lead authority");
    }
    if (link.payload.type === "grant-operator") {
      grants.set(link.payload.granteePubkey, {
        actorPubkey: link.payload.granteePubkey,
        grantEventRef: link.eventId,
        maySteer: true,
      });
    } else if (link.payload.type === "grant-viewer") {
      grants.set(link.payload.granteePubkey, {
        actorPubkey: link.payload.granteePubkey,
        grantEventRef: link.eventId,
        maySteer: false,
      });
    } else if (link.payload.type === "revoke") {
      if (!grants.delete(link.payload.granteePubkey)) {
        return fail("revoke names no active grant");
      }
    } else if (link.payload.type === "grant-seat" && link.payload.role) {
      seats.set(link.payload.granteePubkey, {
        actorPubkey: link.payload.granteePubkey,
        role: link.payload.role,
        grantEventRef: link.eventId,
      });
    } else if (link.payload.type === "revoke-seat" && link.payload.role) {
      const seat = seats.get(link.payload.granteePubkey);
      if (!seat) return fail("revoke-seat names no active seat");
      if (seat.role !== link.payload.role)
        return fail("revoke-seat role does not match");
      seats.delete(link.payload.granteePubkey);
    }
    acceptedEventIds.push(link.eventId);
    headEventId = link.eventId;
  }
  return {
    ok: true,
    value: {
      channelRef: input.channelRef,
      genesisRef: input.genesisRef,
      founderPubkey: input.founderPubkey,
      relayPubkey: input.relayPubkey,
      headEventId,
      headSeq: acceptedEventIds.length,
      acceptedEventIds,
      activeGrants: [...grants.values()].sort((a, b) =>
        a.actorPubkey.localeCompare(b.actorPubkey),
      ),
      activeSeats: [...seats.values()].sort((a, b) =>
        a.actorPubkey.localeCompare(b.actorPubkey),
      ),
    },
  };
}
