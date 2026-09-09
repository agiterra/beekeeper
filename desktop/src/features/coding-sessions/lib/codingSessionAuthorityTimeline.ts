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
import { parseExactTags } from "./codingSessionWireDecode";

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

/** Complete transition vocabulary understood by the accepted-chain verifier. */
export type CodingSessionAuthorityTransitionType =
  | "grant-operator"
  | "grant-viewer"
  | "revoke"
  | "grant-seat"
  | "revoke-seat"
  | "takeover"
  | "transfer";

const ACCEPTED_TRANSITION_TYPES = new Set<CodingSessionAuthorityTransitionType>(
  [
    "grant-operator",
    "grant-viewer",
    "revoke",
    "grant-seat",
    "revoke-seat",
    // The two handover links (`docs/HANDOVER_IMPL.md` §1). Verified and
    // advancing the accepted head exactly like the seat links before them,
    // and — also like the seat links — outside this legacy operator/viewer
    // display projection: who *holds* the session is the claim fold's
    // answer (`codingSessionMissionAuthority.ts`), not a roster role. Adding
    // them here is not cosmetic: this analysis fails closed on an unknown
    // type, so without them one accepted takeover would freeze the roster's
    // head and every later grant would look unaccepted.
    "takeover",
    "transfer",
  ],
);
const LEGACY_TRANSITION_FIELDS = [
  "genesisRef",
  "prevAccepted",
  "seq",
  "type",
  "granteePubkey",
] as const;
const SEAT_TRANSITION_FIELDS = [...LEGACY_TRANSITION_FIELDS, "role"] as const;
/** A claim link names the body it will run on, never a role. */
const CLAIM_TRANSITION_FIELDS = [
  ...LEGACY_TRANSITION_FIELDS,
  "bodyPubkey",
] as const;
const LEGACY_RECEIPT_FIELDS = [
  "type",
  "genesisRef",
  "acceptedEventId",
  "seq",
  "transitionType",
  "granteePubkey",
] as const;
const SEAT_RECEIPT_FIELDS = [...LEGACY_RECEIPT_FIELDS, "role"] as const;
/** The relay carries `bodyPubkey` on a claim receipt when it has one. */
const CLAIM_RECEIPT_FIELDS = [...LEGACY_RECEIPT_FIELDS, "bodyPubkey"] as const;

/** Strictly decoded transition retained for the roster's pending projection. */
export type ParsedCodingSessionAuthorityTransition = {
  eventId: string;
  signerPubkey: string;
  channelId: string;
  seq: number;
  prevAccepted: string | null;
  type: CodingSessionAuthorityTransitionType;
  granteePubkey: string;
  role: string | null;
  /** The execution body a `takeover`/`transfer` claims; null otherwise. */
  bodyPubkey: string | null;
};

type ParsedReceipt = {
  receiptEventId: string;
  acceptedAt: number;
  channelId: string;
  seq: number;
  transitionType: CodingSessionAuthorityTransitionType;
  granteePubkey: string;
  role: string | null;
  bodyPubkey: string | null;
};

/** One fully verified, relay-accepted authority-chain link. */
export type AcceptedCodingSessionAuthorityLink = {
  transitionEventId: string;
  receiptEventId: string;
  acceptedAt: number;
  seq: number;
  type: CodingSessionAuthorityTransitionType;
  granteePubkey: string;
  role: string | null;
  bodyPubkey: string | null;
};

/** Whether the supplied history proves one unambiguous contiguous chain. */
export type CodingSessionAuthorityTimelineDisposition =
  | "complete"
  | "incomplete"
  | "conflicted";

/** Strict receipt-backed authority history, in sequence order. */
export type CodingSessionAuthorityTimeline = {
  disposition: CodingSessionAuthorityTimelineDisposition;
  links: readonly AcceptedCodingSessionAuthorityLink[];
  reason: string | null;
};

/** Internal-grain analysis shared by the roster and sealed timeline facade. */
export type CodingSessionAuthorityChainAnalysis =
  CodingSessionAuthorityTimeline & {
    parsedTransitions: readonly ParsedCodingSessionAuthorityTransition[];
    receiptBackedTransitionIds: ReadonlySet<string>;
  };

function isAcceptedTransitionType(
  value: unknown,
): value is CodingSessionAuthorityTransitionType {
  return (
    typeof value === "string" &&
    ACCEPTED_TRANSITION_TYPES.has(value as CodingSessionAuthorityTransitionType)
  );
}

function isSeatTransitionType(
  value: CodingSessionAuthorityTransitionType,
): value is "grant-seat" | "revoke-seat" {
  return value === "grant-seat" || value === "revoke-seat";
}

function isClaimTransitionType(
  value: CodingSessionAuthorityTransitionType,
): value is "takeover" | "transfer" {
  return value === "takeover" || value === "transfer";
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
): ParsedCodingSessionAuthorityTransition | null {
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
  const claimTransition = isClaimTransitionType(payload.type);
  if (
    !hasExactFields(payload, [
      seatTransition
        ? SEAT_TRANSITION_FIELDS
        : claimTransition
          ? CLAIM_TRANSITION_FIELDS
          : LEGACY_TRANSITION_FIELDS,
    ]) ||
    payload.genesisRef !== genesisRef ||
    (claimTransition &&
      (typeof payload.bodyPubkey !== "string" ||
        !HEX64_REGEX.test(payload.bodyPubkey)))
  ) {
    return null;
  }
  const seq = payload.seq;
  const prevAccepted = payload.prevAccepted;
  if (
    !isU32Sequence(seq) ||
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
    signerPubkey: event.pubkey,
    channelId: tagValues[0],
    seq,
    prevAccepted: prevAccepted as string | null,
    type: payload.type,
    granteePubkey: payload.granteePubkey,
    role: seatTransition ? (payload.role as string) : null,
    bodyPubkey: claimTransition ? (payload.bodyPubkey as string) : null,
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
    !Number.isSafeInteger(event.created_at) ||
    event.created_at < 0 ||
    !hasValidSignature(event)
  ) {
    return null;
  }
  const tagValues = parseExactTags(event.tags, ["h"]);
  if (!tagValues || tagValues[0] !== expectedChannel) return null;
  const payload = parseJsonObject(event.content);
  if (!payload || !isAcceptedTransitionType(payload.transitionType))
    return null;
  const seatTransition = isSeatTransitionType(payload.transitionType);
  const claimTransition = isClaimTransitionType(payload.transitionType);
  if (
    !hasExactFields(
      payload,
      seatTransition
        ? [SEAT_RECEIPT_FIELDS]
        : claimTransition
          ? // Present-or-absent: the relay writes `bodyPubkey` "when
            // present", and either form binds the transition it names.
            [LEGACY_RECEIPT_FIELDS, CLAIM_RECEIPT_FIELDS]
          : [LEGACY_RECEIPT_FIELDS],
    ) ||
    (claimTransition &&
      Object.hasOwn(payload, "bodyPubkey") &&
      (typeof payload.bodyPubkey !== "string" ||
        !HEX64_REGEX.test(payload.bodyPubkey))) ||
    payload.type !== CODING_SESSION_AUTHORITY_RECEIPT_TYPE ||
    payload.genesisRef !== genesisRef ||
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
      receiptEventId: event.id,
      acceptedAt: event.created_at,
      channelId: tagValues[0],
      seq: payload.seq,
      transitionType: payload.transitionType,
      granteePubkey: payload.granteePubkey,
      role: seatTransition ? (payload.role as string) : null,
      bodyPubkey:
        claimTransition && typeof payload.bodyPubkey === "string"
          ? payload.bodyPubkey
          : null,
    },
  };
}

function isMalformedScopedReceiptClaim(
  event: RelayEvent,
  genesisRef: string,
  expectedChannel: string,
  trustedRelayPubkey: string,
): boolean {
  if (
    event.kind !== KIND_SYSTEM_MESSAGE ||
    event.pubkey !== trustedRelayPubkey ||
    !hasValidSignature(event)
  ) {
    return false;
  }
  const tagValues = parseExactTags(event.tags, ["h"]);
  if (!tagValues || tagValues[0] !== expectedChannel) return false;
  const payload = parseJsonObject(event.content);
  return (
    payload?.type === CODING_SESSION_AUTHORITY_RECEIPT_TYPE &&
    payload.genesisRef === genesisRef
  );
}

function receiptBindsTransition(
  receipt: ParsedReceipt,
  transition: ParsedCodingSessionAuthorityTransition,
): boolean {
  return (
    receipt.channelId === transition.channelId &&
    receipt.seq === transition.seq &&
    receipt.transitionType === transition.type &&
    receipt.granteePubkey === transition.granteePubkey &&
    receipt.role === transition.role &&
    // An absent `bodyPubkey` leaves the signed transition as the only
    // statement of the body; a *different* one is not evidence of anything.
    (receipt.bodyPubkey === null ||
      receipt.bodyPubkey === transition.bodyPubkey)
  );
}

function authorityLink(
  transition: ParsedCodingSessionAuthorityTransition,
  receipt: ParsedReceipt,
): AcceptedCodingSessionAuthorityLink {
  return {
    transitionEventId: transition.eventId,
    receiptEventId: receipt.receiptEventId,
    acceptedAt: receipt.acceptedAt,
    seq: transition.seq,
    type: transition.type,
    granteePubkey: transition.granteePubkey,
    role: transition.role,
    bodyPubkey: transition.bodyPubkey,
  };
}

/** Parse and verify the single contiguous accepted chain in an event window. */
export function analyzeCodingSessionAuthorityChain(input: {
  expectedChannel: string;
  trustedRelayPubkey: string;
  genesisRef: string;
  founderPubkey: string | null;
  transitions: readonly RelayEvent[];
  receipts: readonly RelayEvent[];
}): CodingSessionAuthorityChainAnalysis {
  const uniqueTransitions = new Map<string, RelayEvent>();
  for (const event of input.transitions) {
    if (!uniqueTransitions.has(event.id))
      uniqueTransitions.set(event.id, event);
  }
  const uniqueReceipts = new Map<string, RelayEvent>();
  for (const event of input.receipts) {
    if (!uniqueReceipts.has(event.id)) uniqueReceipts.set(event.id, event);
  }

  const receiptsByAcceptedId = new Map<string, ParsedReceipt[]>();
  let incompleteReason: string | null = null;
  for (const event of uniqueReceipts.values()) {
    const parsed = parseReceipt(
      event,
      input.genesisRef,
      input.expectedChannel,
      input.trustedRelayPubkey,
    );
    if (!parsed) {
      if (
        isMalformedScopedReceiptClaim(
          event,
          input.genesisRef,
          input.expectedChannel,
          input.trustedRelayPubkey,
        )
      ) {
        incompleteReason ??= `Relay authority receipt ${event.id} was malformed.`;
      }
      continue;
    }
    const receipts = receiptsByAcceptedId.get(parsed.acceptedEventId) ?? [];
    receipts.push(parsed.receipt);
    receiptsByAcceptedId.set(parsed.acceptedEventId, receipts);
  }
  const receiptBackedTransitionIds = new Set(receiptsByAcceptedId.keys());

  const parsedTransitions: ParsedCodingSessionAuthorityTransition[] = [];
  const transitionsById = new Map<
    string,
    ParsedCodingSessionAuthorityTransition
  >();
  for (const event of uniqueTransitions.values()) {
    const parsed = parseTransition(
      event,
      input.genesisRef,
      input.expectedChannel,
    );
    if (!parsed) continue;
    parsedTransitions.push(parsed);
    transitionsById.set(parsed.eventId, parsed);
  }

  const pairedBySeq = new Map<
    number,
    {
      transition: ParsedCodingSessionAuthorityTransition;
      receipt: ParsedReceipt;
    }[]
  >();
  const problemBySeq = new Map<
    number,
    { disposition: CodingSessionAuthorityTimelineDisposition; reason: string }
  >();
  for (const [acceptedEventId, receipts] of receiptsByAcceptedId) {
    const transition = transitionsById.get(acceptedEventId);
    if (!transition) {
      incompleteReason ??= `Accepted transition ${acceptedEventId} was not present in the supplied history.`;
      continue;
    }
    if (receipts.length !== 1) {
      problemBySeq.set(transition.seq, {
        disposition: "conflicted",
        reason: `More than one relay receipt claims accepted transition ${acceptedEventId}.`,
      });
      continue;
    }
    const receipt = receipts[0];
    if (!receiptBindsTransition(receipt, transition)) {
      problemBySeq.set(transition.seq, {
        disposition: "conflicted",
        reason: `Relay receipt ${receipt.receiptEventId} does not bind the exact facts of transition ${acceptedEventId}.`,
      });
      continue;
    }
    const legacyOwnerOnly =
      transition.type === "grant-operator" ||
      transition.type === "grant-viewer" ||
      transition.type === "revoke";
    if (
      legacyOwnerOnly &&
      input.founderPubkey !== null &&
      transition.signerPubkey !== input.founderPubkey
    ) {
      problemBySeq.set(transition.seq, {
        disposition: "conflicted",
        reason: `Legacy authority transition ${acceptedEventId} was not signed by the session founder.`,
      });
      continue;
    }
    const candidates = pairedBySeq.get(transition.seq) ?? [];
    candidates.push({ transition, receipt });
    pairedBySeq.set(transition.seq, candidates);
  }
  for (const [seq, candidates] of pairedBySeq) {
    if (candidates.length > 1) {
      problemBySeq.set(seq, {
        disposition: "conflicted",
        reason: `More than one relay-accepted transition claims authority sequence ${seq}.`,
      });
    }
  }

  const links: AcceptedCodingSessionAuthorityLink[] = [];
  const activeSeats = new Map<string, string>();
  let previousAccepted: string | null = null;
  const finish = (
    disposition: CodingSessionAuthorityTimelineDisposition,
    reason: string | null,
  ): CodingSessionAuthorityChainAnalysis => ({
    disposition,
    links,
    reason,
    parsedTransitions,
    receiptBackedTransitionIds,
  });
  const maxSeq = Math.max(0, ...pairedBySeq.keys(), ...problemBySeq.keys());
  for (let seq = 1; seq <= maxSeq; seq += 1) {
    const problem = problemBySeq.get(seq);
    if (problem) return finish(problem.disposition, problem.reason);
    const pair = pairedBySeq.get(seq)?.[0];
    if (!pair) {
      return finish(
        "incomplete",
        incompleteReason ??
          `Authority sequence ${seq} was missing from the supplied history.`,
      );
    }
    if (pair.transition.prevAccepted !== previousAccepted) {
      return finish(
        "conflicted",
        `Authority transition ${pair.transition.eventId} does not extend the previous accepted head.`,
      );
    }
    if (
      pair.transition.type === "grant-seat" &&
      pair.transition.role !== null
    ) {
      activeSeats.set(pair.transition.granteePubkey, pair.transition.role);
    } else if (pair.transition.type === "revoke-seat") {
      if (
        pair.transition.role === null ||
        activeSeats.get(pair.transition.granteePubkey) !== pair.transition.role
      ) {
        return finish(
          "conflicted",
          `Seat revocation ${pair.transition.eventId} does not match an active accepted seat.`,
        );
      }
      activeSeats.delete(pair.transition.granteePubkey);
    }
    links.push(authorityLink(pair.transition, pair.receipt));
    previousAccepted = pair.transition.eventId;
  }
  return incompleteReason === null
    ? finish("complete", null)
    : finish("incomplete", incompleteReason);
}

/** Verify one session's complete accepted authority timeline. */
export function buildCodingSessionAcceptedAuthorityTimeline(input: {
  expectedChannel: string;
  trustedRelayPubkey: string;
  genesisRef: string;
  founderPubkey: string;
  transitions: readonly RelayEvent[];
  receipts: readonly RelayEvent[];
}): CodingSessionAuthorityTimeline {
  const analysis = analyzeCodingSessionAuthorityChain(input);
  return {
    disposition: analysis.disposition,
    links: analysis.links,
    reason: analysis.reason,
  };
}
