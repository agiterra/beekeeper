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
/** Mirrors core's `MAX_PROJECT_REF_BYTES`. */
const MAX_PROJECT_REF_BYTES = 256;
const encoder = new TextEncoder();

export type CodingSessionAuthorityTransitionType =
  | "grant-operator"
  | "grant-viewer"
  | "revoke"
  | "grant-seat"
  | "revoke-seat"
  | "takeover"
  | "transfer"
  // The project-action delegation and its withdrawal (ledger 186). This
  // decoder fails closed on an unknown type, so leaving them out did not
  // make the chain narrower — it made every chain that carries one
  // undecodable, which is what blanked Mission, Decisions and Settlement for
  // every owner-founded team session (ledger 238).
  | "grant-project-actions"
  | "revoke-project-actions";

type AuthorityTransition = {
  genesisRef: string;
  prevAccepted: string | null;
  seq: number;
  type: CodingSessionAuthorityTransitionType;
  granteePubkey: string;
  role?: string;
  bodyPubkey?: string;
  projectRef?: string;
};

type AuthorityReceipt = {
  type: typeof RECEIPT_TYPE;
  genesisRef: string;
  acceptedEventId: string;
  seq: number;
  transitionType: CodingSessionAuthorityTransitionType;
  granteePubkey: string;
  role?: string;
  bodyPubkey?: string;
  projectRef?: string;
};

/**
 * One accepted handover claim — the TypeScript twin of `CurrentClaim`
 * (`crates/buzz-core/src/coding_session_authority_claim.rs`), field for field.
 *
 * Deliberately carries no timestamp: the Rust twin does not, because a
 * consumer that folded the chain from receipts alone knows the claim without
 * knowing when it landed. The time is reported beside the state, where `null`
 * can mean "unknown" without pretending to be "just now".
 */
export type CodingSessionClaim = {
  /** The participant who now owns this session's work. */
  claimant: string;
  /** The provider authority pubkey of the body they will work on. */
  bodyPubkey: string;
  acceptedEventId: string;
  seq: number;
};

/**
 * Claim state: three values, and consumers must keep them apart.
 *
 * `none` is "no handover ever happened, existing rules apply". `voided` is
 * **not** the same thing: a handover happened and its claimant lost standing,
 * so the fence stays up for every body until somebody with standing takes the
 * session over again. Collapsing `voided` into `none` would silently re-open a
 * session that a revoke deliberately froze, and a regrant of the same pubkey
 * never restores the old claim (`docs/HANDOVER_IMPL.md` §1).
 */
export type CodingSessionClaimState =
  | { state: "no-claim" }
  | ({ state: "active" } & CodingSessionClaim)
  | {
      state: "voided";
      /** The claim that was voided — still the name a reader shows. */
      last: CodingSessionClaim;
      /** The accepted revoke or demotion that voided it. */
      voidedBy: string;
      seq: number;
    };

/** The claim state of a chain no handover link has ever touched. */
export const CODING_SESSION_NO_CLAIM: CodingSessionClaimState = Object.freeze({
  state: "no-claim",
});

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
  /**
   * Every accepted transition, in accepted order, with the **receipt's** own
   * `created_at`.
   *
   * `activeGrants` answers "who may steer now"; the policy fold asks "who
   * could steer when this record was published", which is a different
   * question and needs the times. Carried here rather than recomputed
   * anywhere else, because two projections of one chain is exactly the defect
   * REVIEW-B2 F1 named.
   */
  policyGrants: Array<{
    /**
     * Event id of the signed kind-44228 this entry was read from.
     *
     * REVIEW-L2 F15: the native policy fold holds each claimed grant against
     * the signed transition it names, so this projection can only ever fail to
     * see a grant — never invent one.
     */
    transitionEventId: string;
    grantee: string;
    acceptedAt: number;
    transitionType: CodingSessionAuthorityTransitionType;
  }>;
  /**
   * Who owns this session's work, from the §1 rule over the same chain.
   *
   * Umbrella-wide by construction: the chain is rooted at the genesis, so one
   * accepted claim hands over the whole session — every execution and every
   * assignment under it — and no surface may describe it as moving a slice.
   */
  claim: CodingSessionClaimState;
  /**
   * Every accepted claim link, in accepted order.
   *
   * A continuation names the claim it acted under, and that claim may since
   * have been superseded or voided — "continued by B until …" is history, not
   * a live claim, and a reader that only held the current state would have to
   * call every past continuation unauthorized.
   */
  claimHistory: CodingSessionClaim[];
  /**
   * When the claim in force was accepted, in unix seconds, or `null`.
   *
   * Beside the state rather than inside it, exactly as Rust carries
   * `claim_since` beside `ClaimState`: "unknown" and "just now" must never
   * render alike.
   */
  claimSince: number | null;
  /** When the claim was voided, in unix seconds, or `null`. */
  claimVoidedAt: number | null;
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
    value === "revoke-seat" ||
    value === "takeover" ||
    value === "transfer" ||
    value === "grant-project-actions" ||
    value === "revoke-project-actions"
  );
}

/** The two links that move the claim. Both carry `bodyPubkey`, never `role`. */
function isClaimTransitionType(value: unknown): boolean {
  return value === "takeover" || value === "transfer";
}

/** The two links that delegate a project's actions. Both carry `projectRef`. */
function isProjectActionsTransitionType(value: unknown): boolean {
  return (
    value === "grant-project-actions" || value === "revoke-project-actions"
  );
}

/**
 * Whether `value` is a project coordinate a delegation may name:
 * `30621:<64-hex owner>:<non-empty d>`, within `MAX_PROJECT_REF_BYTES`.
 *
 * Kind 30621 is the project record. A coordinate of any other kind is refused
 * rather than normalized (core's `validate_project_ref`), so a delegation can
 * never be read as filed against a repository or a pack coordinate instead.
 */
function isProjectRefCoordinate(value: unknown): value is string {
  if (
    typeof value !== "string" ||
    encoder.encode(value).length > MAX_PROJECT_REF_BYTES
  ) {
    return false;
  }
  const first = value.indexOf(":");
  const second = value.indexOf(":", first + 1);
  if (first === -1 || second === -1) return false;
  return (
    value.slice(0, first) === "30621" &&
    HEX64.test(value.slice(first + 1, second)) &&
    value.slice(second + 1).trim().length > 0
  );
}

const BASE_TRANSITION_FIELDS = [
  "genesisRef",
  "prevAccepted",
  "seq",
  "type",
  "granteePubkey",
] as const;

function decodeTransitionContent(
  source: string,
): StrictDecodeResult<AuthorityTransition> {
  const value = parseJson(source, 512);
  const type =
    typeof value === "object" && value !== null
      ? (value as Record<string, unknown>).type
      : null;
  const isSeat = type === "grant-seat" || type === "revoke-seat";
  const isClaim = isClaimTransitionType(type);
  const isProjectActions = isProjectActionsTransitionType(type);
  if (
    !hasExactFields(value, [
      isSeat
        ? [...BASE_TRANSITION_FIELDS, "role"]
        : isClaim
          ? [...BASE_TRANSITION_FIELDS, "bodyPubkey"]
          : isProjectActions
            ? [...BASE_TRANSITION_FIELDS, "projectRef"]
            : [...BASE_TRANSITION_FIELDS],
    ]) ||
    // Required 64-hex for a claim, and absent for everything else: a grant
    // that named a body would be claiming an execution the relay never
    // serialized a claim for.
    (isClaim &&
      (typeof value.bodyPubkey !== "string" ||
        !HEX64.test(value.bodyPubkey))) ||
    // Required on the link, and the exact-field set above is what refuses a
    // `projectRef` on any other type. An unscoped delegation is not a
    // narrower grant; it is one that reaches every project the grantee sees.
    (isProjectActions && !isProjectRefCoordinate(value.projectRef)) ||
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
  const isClaim = isClaimTransitionType(transitionType);
  const isProjectActions = isProjectActionsTransitionType(transitionType);
  const baseReceiptFields = [
    "type",
    "genesisRef",
    "acceptedEventId",
    "seq",
    "transitionType",
    "granteePubkey",
  ];
  if (
    !hasExactFields(
      value,
      isSeat
        ? [[...baseReceiptFields, "role"]]
        : isClaim
          ? // Required, not present-or-absent. A claim link cannot be signed
            // without a body, so the relay cannot emit a claim receipt that
            // drops one; a receipt that did would name a claim no fence could
            // check against its own key. The CLI, the provider and the
            // timeline decoder have always refused it, and the shared vectors
            // in `conformance/authority-chain` are what proved this reader
            // disagreed — the same way they proved it about `projectRef`
            // (ledger 204, ledger 238).
            [[...baseReceiptFields, "bodyPubkey"]]
          : isProjectActions
            ? // Required, not present-or-absent: a delegation link cannot be
              // signed without a scope, so the relay cannot emit a delegation
              // receipt that drops one (`side_effects.rs`, ledger 186), and a
              // receipt that did would say a delegation was accepted without
              // saying what it reaches.
              [[...baseReceiptFields, "projectRef"]]
            : [[...baseReceiptFields]],
    ) ||
    (isClaim &&
      Object.hasOwn(value, "bodyPubkey") &&
      (typeof value.bodyPubkey !== "string" ||
        !HEX64.test(value.bodyPubkey))) ||
    (isProjectActions && !isProjectRefCoordinate(value.projectRef)) ||
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
 * The §1 claim rule, one accepted link at a time.
 *
 * Written as a fold over `ClaimState` rather than as "the newest takeover"
 * because the difference between *no claim* and *a voided claim* is the whole
 * safety property: a revoke or a demotion of the claimant freezes the session
 * for everyone until somebody with standing claims it again, and a regrant of
 * the very same pubkey is explicitly not that act.
 */
function foldClaimLink(
  state: CodingSessionClaimState,
  link: {
    eventId: string;
    payload: AuthorityTransition;
  },
): CodingSessionClaimState {
  const { payload } = link;
  if (isClaimTransitionType(payload.type) && payload.bodyPubkey) {
    return {
      state: "active",
      claimant: payload.granteePubkey,
      bodyPubkey: payload.bodyPubkey,
      acceptedEventId: link.eventId,
      seq: payload.seq,
    };
  }
  if (state.state !== "active") return state;
  const voids =
    (payload.type === "revoke" || payload.type === "grant-viewer") &&
    payload.granteePubkey === state.claimant;
  if (!voids) return state;
  const { state: _state, ...last } = state;
  return {
    state: "voided",
    last,
    voidedBy: link.eventId,
    seq: payload.seq,
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
    {
      eventId: string;
      signer: string;
      acceptedAt: number;
      payload: AuthorityTransition;
    }
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
      receipt.value.role !== payload.role ||
      // A receipt that names a *different* body than the transition it claims
      // to have accepted is not evidence of anything; an absent one leaves the
      // signed transition as the only statement, which is where it came from.
      (receipt.value.bodyPubkey !== undefined &&
        receipt.value.bodyPubkey !== payload.bodyPubkey) ||
      // And the same rule for the delegation's scope: a receipt naming a
      // different project than the link it claims to have accepted would
      // fold a delegation of a project the signer never named.
      receipt.value.projectRef !== payload.projectRef
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
      // The relay receipt's own stamp, not the transition's: acceptance is
      // what put this link in the chain.
      acceptedAt: event.created_at,
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
  let claimState: CodingSessionClaimState = CODING_SESSION_NO_CLAIM;
  const claimHistory: CodingSessionClaim[] = [];
  let claimSince: number | null = null;
  let claimVoidedAt: number | null = null;
  const acceptedEventIds: string[] = [];
  const policyGrants: CodingSessionMissionAuthorityProjection["policyGrants"] =
    [];
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
    const claimTransition = isClaimTransitionType(link.payload.type);
    const currentClaimant =
      claimState.state === "active" ? claimState.claimant : null;
    if (
      (seatTransition &&
        !signerIsFounder &&
        !signerIsOperator &&
        !signerIsLead) ||
      // §1: a takeover is a self-claim by the founder or a live operator; a
      // transfer is signed by the current claimant or the founder and names a
      // grantee who already holds standing. Anything else is not this chain's
      // link, however well the relay serialized it.
      (link.payload.type === "takeover" &&
        (!(signerIsFounder || signerIsOperator) ||
          link.payload.granteePubkey !== link.signer)) ||
      (link.payload.type === "transfer" &&
        (!(signerIsFounder || link.signer === currentClaimant) ||
          !(
            link.payload.granteePubkey === input.founderPubkey ||
            grants.get(link.payload.granteePubkey)?.maySteer === true
          ))) ||
      (!seatTransition && !claimTransition && !signerIsFounder)
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
    const previousClaim = claimState;
    claimState = foldClaimLink(claimState, link);
    if (
      claimState.state === "active" &&
      claimState.acceptedEventId === link.eventId
    ) {
      const { state: _state, ...claim } = claimState;
      claimHistory.push(claim);
      claimSince = link.acceptedAt;
      claimVoidedAt = null;
    } else if (
      claimState.state === "voided" &&
      previousClaim.state === "active"
    ) {
      claimVoidedAt = link.acceptedAt;
    }
    acceptedEventIds.push(link.eventId);
    policyGrants.push({
      transitionEventId: link.eventId,
      grantee: link.payload.granteePubkey,
      acceptedAt: link.acceptedAt,
      transitionType: link.payload.type,
    });
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
      policyGrants,
      claim: claimState,
      claimHistory,
      claimSince,
      claimVoidedAt,
      activeGrants: [...grants.values()].sort((a, b) =>
        a.actorPubkey.localeCompare(b.actorPubkey),
      ),
      activeSeats: [...seats.values()].sort((a, b) =>
        a.actorPubkey.localeCompare(b.actorPubkey),
      ),
    },
  };
}
