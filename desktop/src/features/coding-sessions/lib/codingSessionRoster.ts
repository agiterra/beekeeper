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
import type { EntityRole } from "@/shared/lib/entityRoles";
import {
  analyzeCodingSessionAuthorityChain,
  buildCodingSessionAcceptedAuthorityTimeline,
  CODING_SESSION_AUTHORITY_RECEIPT_TYPE,
  CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION,
  type CodingSessionAuthorityTransitionType,
} from "./codingSessionAuthorityTimeline";

export {
  buildCodingSessionAcceptedAuthorityTimeline,
  CODING_SESSION_AUTHORITY_RECEIPT_TYPE,
  CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION,
};
export type {
  AcceptedCodingSessionAuthorityLink,
  CodingSessionAuthorityTimeline,
  CodingSessionAuthorityTimelineDisposition,
  CodingSessionAuthorityTransitionType,
} from "./codingSessionAuthorityTimeline";

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
/**
 * A project coordinate a delegation may name: `30621:<64-hex owner>:<d>`,
 * mirroring core's `validate_project_ref` (ledger 186, finding 178(f)).
 */
const PROJECT_REF_REGEX = /^30621:[0-9a-f]{64}:\S/;
const ROLE_SLUG_REGEX = /^[a-z0-9-]{1,64}$/;
const MAX_U32 = 0xffff_ffff;

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
  /**
   * Live project-action delegations, by
   * [`codingSessionProjectActionGrantKey`]. Receipt-backed like every other
   * accepted fact here, and separate from both the legacy grant map and the
   * seat map: a delegation is standing over one project's kind:30620 actions
   * and kind:46020 manual runs, and nothing else (ledger 186, finding
   * 178(f)). Without it a caller that just published a delegation had no read
   * that could confirm the relay accepted it.
   */
  projectActionGrants: Map<string, CodingSessionProjectActionGrant>;
};

/** A display row: shared role vocabulary, founder pinned first as owner. */
export type CodingSessionRosterEntry = {
  pubkey: string;
  role: EntityRole;
  pending?: boolean;
  /**
   * The receipt-backed `grant-seat` role slug this key holds in this session.
   * **Absent** — not null — when it holds none.
   *
   * The fold has always computed `activeSeats`; this projection threw it away,
   * so a seated key rendered as an anonymous "Collaborator" with nothing said
   * about the role it was hired into. Purely additive: it admits no row, drops
   * no row, and changes no existing field, and omit-when-absent (the same
   * shape convention as `role`/`bodyPubkey` on the transition payload above)
   * keeps every seatless row byte-identical to what it was. A seat is a
   * session fact and never project membership.
   */
  seatRole?: string | null;
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

function isSeatTransitionType(
  value: CodingSessionAuthorityTransitionType,
): value is "grant-seat" | "revoke-seat" {
  return value === "grant-seat" || value === "revoke-seat";
}

/** The two handover links, which name a body instead of a role (§1). */
function isClaimTransitionType(
  value: CodingSessionAuthorityTransitionType,
): value is "takeover" | "transfer" {
  return value === "takeover" || value === "transfer";
}

/** The two project-action delegation links, which name a project (ledger 186). */
function isProjectActionsTransitionType(
  value: CodingSessionAuthorityTransitionType,
): value is "grant-project-actions" | "revoke-project-actions" {
  return (
    value === "grant-project-actions" || value === "revoke-project-actions"
  );
}

/**
 * One live project-action delegation, keyed in the fold by grantee+project.
 *
 * `grantedBy` is kept because a consumer re-checks that pubkey against the
 * project's *current* owners: a granter who has since lost ownership must not
 * leave a capability behind (`coding_session_project_action_grant.rs`).
 */
export type CodingSessionProjectActionGrant = {
  granteePubkey: string;
  projectRef: string;
  grantedBy: string;
  grantEventId: string;
};

/** The fold's key for one (grantee, project) delegation pair. */
export function codingSessionProjectActionGrantKey(
  granteePubkey: string,
  projectRef: string,
): string {
  return `${granteePubkey.toLowerCase()}|${projectRef}`;
}

function isU32Sequence(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isInteger(value) &&
    value >= 1 &&
    value <= MAX_U32
  );
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
  const analysis = analyzeCodingSessionAuthorityChain({
    ...input,
    founderPubkey: null,
  });

  const accepted = new Map<string, CodingSessionRosterRole>();
  const activeSeats = new Map<string, string>();
  const projectActionGrants = new Map<
    string,
    CodingSessionProjectActionGrant
  >();
  let acceptedHead: { eventId: string; seq: number } | null = null;
  for (const link of analysis.links) {
    if (link.type === "revoke") {
      accepted.delete(link.granteePubkey);
    } else if (link.type === "revoke-seat") {
      if (
        link.role === null ||
        activeSeats.get(link.granteePubkey) !== link.role
      ) {
        break; // The strict analysis stops before this link.
      }
      activeSeats.delete(link.granteePubkey);
    } else if (link.type === "grant-seat" && link.role !== null) {
      activeSeats.set(link.granteePubkey, link.role);
    } else if (isProjectActionsTransitionType(link.type)) {
      // A later link naming the same (grantee, project) pair supersedes the
      // earlier one, and a revocation removes it — the same fold core runs.
      if (link.projectRef !== null) {
        const key = codingSessionProjectActionGrantKey(
          link.granteePubkey,
          link.projectRef,
        );
        if (link.type === "grant-project-actions") {
          projectActionGrants.set(key, {
            granteePubkey: link.granteePubkey,
            projectRef: link.projectRef,
            grantedBy: link.signerPubkey,
            grantEventId: link.transitionEventId,
          });
        } else {
          projectActionGrants.delete(key);
        }
      }
    } else if (!isSeatTransitionType(link.type)) {
      const role = rosterRoleForGrantType(link.type);
      if (role) accepted.set(link.granteePubkey, role);
    }
    acceptedHead = { eventId: link.transitionEventId, seq: link.seq };
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
  for (const transition of analysis.parsedTransitions) {
    if (transition.seq <= headSeq) continue;
    if (analysis.receiptBackedTransitionIds.has(transition.eventId)) continue;
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

  return {
    accepted,
    activeSeats,
    acceptedHead,
    pending,
    projectActionGrants,
  };
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
  const seatRoleFor = (pubkey: string) => {
    const seatRole = fold.activeSeats.get(pubkey);
    return seatRole === undefined ? {} : { seatRole };
  };
  const entries: CodingSessionRosterEntry[] = founder
    ? [{ pubkey: founder, role: "owner", ...seatRoleFor(founder) }]
    : [];
  for (const [pubkey, role] of fold.accepted) {
    if (pubkey === founder) continue;
    entries.push({
      pubkey,
      role: entityRoleForRosterRole(role),
      ...seatRoleFor(pubkey),
    });
  }
  const listed = new Set(entries.map((entry) => entry.pubkey));
  for (const invite of fold.pending) {
    if (listed.has(invite.pubkey)) continue;
    listed.add(invite.pubkey);
    entries.push({
      pubkey: invite.pubkey,
      role: entityRoleForRosterRole(invite.role),
      pending: true,
      ...seatRoleFor(invite.pubkey),
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
  bodyPubkey?: string;
  projectRef?: string;
}): string {
  return JSON.stringify({
    genesisRef: input.genesisRef,
    prevAccepted: input.prevAccepted,
    seq: input.seq,
    type: input.type,
    granteePubkey: input.granteePubkey,
    ...(isSeatTransitionType(input.type) ? { role: input.role } : {}),
    // Trailing and type-scoped, exactly like `role`: every earlier form stays
    // byte-identical, and a claim link carries the body it will run on.
    ...(isClaimTransitionType(input.type)
      ? { bodyPubkey: input.bodyPubkey }
      : {}),
    // Trailing and type-scoped for the same reason: a delegation carries the
    // one project it delegates, and no earlier form gains a key.
    ...(isProjectActionsTransitionType(input.type)
      ? { projectRef: input.projectRef }
      : {}),
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
  /** Required for `takeover`/`transfer`, refused for every other type. */
  bodyPubkey?: string;
  /**
   * Required for `grant-project-actions`/`revoke-project-actions`, refused
   * for every other type (ledger 186, finding 178(f)).
   */
  projectRef?: string;
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
  const claimTransition = isClaimTransitionType(input.type);
  const projectActionsTransition = isProjectActionsTransitionType(input.type);
  if (
    (seatTransition &&
      (typeof input.role !== "string" || !ROLE_SLUG_REGEX.test(input.role))) ||
    (!seatTransition && input.role !== undefined)
  ) {
    throw new Error(
      "role must be one lowercase role slug exactly for a seat transition",
    );
  }
  if (
    (claimTransition &&
      (typeof input.bodyPubkey !== "string" ||
        !HEX64_REGEX.test(input.bodyPubkey.toLowerCase()))) ||
    (!claimTransition && input.bodyPubkey !== undefined)
  ) {
    throw new Error(
      "bodyPubkey must be one lowercase 64-hex provider authority pubkey exactly for a takeover or transfer",
    );
  }
  if (
    (projectActionsTransition &&
      (typeof input.projectRef !== "string" ||
        !PROJECT_REF_REGEX.test(input.projectRef))) ||
    (!projectActionsTransition && input.projectRef !== undefined)
  ) {
    throw new Error(
      "projectRef must be 30621:<64-hex owner>:<d> exactly for a project-actions grant or revocation",
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
      ...(claimTransition
        ? { bodyPubkey: input.bodyPubkey?.toLowerCase() }
        : {}),
      ...(projectActionsTransition ? { projectRef: input.projectRef } : {}),
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
    /** The body a `takeover`/`transfer` claims (§1). */
    bodyPubkey?: string;
    /** The project a `grant-project-actions`/`revoke-project-actions` names. */
    projectRef?: string;
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
        ...(input.bodyPubkey !== undefined
          ? { bodyPubkey: input.bodyPubkey }
          : {}),
        ...(input.projectRef !== undefined
          ? { projectRef: input.projectRef }
          : {}),
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
