/**
 * Recorded-timeline commissioning proof for Roles.
 *
 * This adapter deliberately accepts raw signed relay events, never a caller-
 * supplied set of "accepted" grants. The roster module verifies the exact
 * transition and receipt envelopes, signatures, relay signer, fact binding,
 * founder signer constraint, sequence, predecessor links, and seat state
 * before this module can evaluate a lifecycle command.
 */

import {
  buildCodingSessionAcceptedAuthorityTimeline,
  type CodingSessionAuthorityTimelineDisposition,
  type AcceptedCodingSessionAuthorityLink,
} from "@/features/coding-sessions/lib/codingSessionAuthorityTimeline";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_AUTHORITY_TRANSITION,
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_SYSTEM_MESSAGE,
} from "@/shared/constants/kinds";
import { hasValidSignature } from "@/shared/lib/authors";

const HEX64_REGEX = /^[0-9a-f]{64}$/;

/** Exact umbrella boundary against which authority evidence was verified. */
export type RoleOperatorAuthorityScope = {
  channelId: string;
  genesisRef: string;
  founderPubkey: string;
};

/** A reusable, strict authority projection for one exact session genesis. */
export type RoleOperatorAuthorityEvidence = {
  disposition: CodingSessionAuthorityTimelineDisposition;
  timeline: readonly AcceptedCodingSessionAuthorityLink[];
  reason: string | null;
  scope: RoleOperatorAuthorityScope;
};

/** Why an exact lifecycle command signer was allowed to commission work. */
export type RoleOperatorAuthorization = {
  authorized: boolean;
  basis: "founder" | "operator" | null;
  reason: string | null;
};

function incompleteEvidence(
  scope: RoleOperatorAuthorityScope,
  reason: string,
): RoleOperatorAuthorityEvidence {
  return {
    disposition: "incomplete",
    timeline: [],
    reason,
    scope,
  };
}

/**
 * Build one receipt-backed authority timeline for reuse across every command
 * in a generation lineage. Malformed input becomes unavailable evidence; it
 * never throws and never exposes a partially verified grant as authority.
 */
export function buildRoleOperatorAuthorityEvidence(input: {
  events: readonly RelayEvent[];
  trustedRelayPubkey: string | null;
  scope: RoleOperatorAuthorityScope;
  sourceComplete: boolean;
}): RoleOperatorAuthorityEvidence {
  const { scope } = input;
  if (
    scope.channelId.length === 0 ||
    !HEX64_REGEX.test(scope.genesisRef) ||
    !HEX64_REGEX.test(scope.founderPubkey)
  ) {
    return incompleteEvidence(
      scope,
      "The session authority scope was malformed.",
    );
  }
  if (
    input.trustedRelayPubkey === null ||
    !HEX64_REGEX.test(input.trustedRelayPubkey)
  ) {
    return incompleteEvidence(
      scope,
      "The active relay did not advertise a trusted signing key.",
    );
  }

  const timeline = buildCodingSessionAcceptedAuthorityTimeline({
    expectedChannel: scope.channelId,
    trustedRelayPubkey: input.trustedRelayPubkey,
    genesisRef: scope.genesisRef,
    founderPubkey: scope.founderPubkey,
    transitions: input.events.filter(
      (event) => event.kind === KIND_CODING_SESSION_AUTHORITY_TRANSITION,
    ),
    receipts: input.events.filter(
      (event) => event.kind === KIND_SYSTEM_MESSAGE,
    ),
  });
  if (timeline.disposition === "conflicted") {
    return {
      disposition: "conflicted",
      timeline: timeline.links,
      reason: timeline.reason,
      scope,
    };
  }
  if (!input.sourceComplete || timeline.disposition === "incomplete") {
    return {
      disposition: "incomplete",
      timeline: timeline.links,
      reason:
        timeline.reason ??
        "The authority-history read was incomplete for this session.",
      scope,
    };
  }
  return {
    disposition: "complete",
    timeline: timeline.links,
    reason: null,
    scope,
  };
}

function commandNamesExactlyOneChannel(
  command: RelayEvent,
  expectedChannel: string,
): boolean {
  const channels = command.tags.filter((tag) => tag[0] === "h");
  return (
    channels.length === 1 &&
    channels[0].length === 2 &&
    channels[0][1] === expectedChannel
  );
}

/**
 * Evaluate one exact, signed lifecycle command at its own `created_at`.
 *
 * Founder authority is intrinsic to the verified genesis and does not depend
 * on relay identity or authority-history availability. Non-founder authority
 * follows `signer_may_steer_at`: verified links stay in sequence order, each
 * link takes effect when its relay receipt timestamp is `<=` the command
 * timestamp, viewer/revoke disable, and seat transitions confer no authority.
 */
export function authorizeRoleOperatorCommand(
  evidence: RoleOperatorAuthorityEvidence,
  command: RelayEvent,
): RoleOperatorAuthorization {
  if (
    command.kind !== KIND_CODING_SESSION_LIFECYCLE_COMMAND ||
    !HEX64_REGEX.test(command.id) ||
    !HEX64_REGEX.test(command.pubkey) ||
    !Number.isSafeInteger(command.created_at) ||
    command.created_at < 0 ||
    !hasValidSignature(command) ||
    !commandNamesExactlyOneChannel(command, evidence.scope.channelId)
  ) {
    return {
      authorized: false,
      basis: null,
      reason:
        "The lifecycle command was not a valid signed event in this session channel.",
    };
  }

  if (command.pubkey === evidence.scope.founderPubkey) {
    return { authorized: true, basis: "founder", reason: null };
  }
  if (evidence.disposition !== "complete") {
    return {
      authorized: false,
      basis: null,
      reason:
        evidence.reason ??
        "The accepted authority timeline was unavailable for this command.",
    };
  }

  let active = false;
  for (const link of evidence.timeline) {
    if (
      link.granteePubkey !== command.pubkey ||
      link.acceptedAt > command.created_at
    ) {
      continue;
    }
    if (link.type === "grant-operator") active = true;
    else if (link.type === "grant-viewer" || link.type === "revoke") {
      active = false;
    }
  }
  return active
    ? { authorized: true, basis: "operator", reason: null }
    : {
        authorized: false,
        basis: null,
        reason:
          "No relay-accepted operator grant was active when this command was signed.",
      };
}
