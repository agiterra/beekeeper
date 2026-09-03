/**
 * What the Context tab says about this umbrella's session policy.
 *
 * Item 107 shipped `fold_coding_session_policies` and one standing rule, and
 * the only Desktop consumer was the launch form — so a founder could publish a
 * 44245 and then have no way, inside the app, to read back what was in force.
 * This module is the reading half, and it decides nothing:
 *
 * - **Which record won is Rust's answer**, through the fold command. There is
 *   no newest-wins comparison here, no grant history, no standing check.
 * - **Three outcomes are three different sentences.** No record, a withdrawal,
 *   and a record are distinct facts; collapsing a withdrawal into "none" would
 *   erase a decision somebody made.
 * - **`budget.turns` is the only field anything enforces** (POLICY.md §4), so
 *   it is the only row that may claim to be enforced, and no field gets a bar,
 *   a meter or a progress ring — nothing is counting them.
 */
import {
  codingSessionPolicyFacts,
  CODING_SESSION_POLICY_STATED_NOT_ENFORCED,
  decodeCodingSessionPolicyRecord,
  type CodingSessionPolicyFact,
  type CodingSessionPolicyRecord,
} from "./codingSessionPolicy";
import { hasExactFields } from "@/shared/coordination/sessionCoordinationStrictJson";
import { truncatePubkey } from "@/shared/lib/pubkey";

export const CODING_SESSION_POLICY_FOLD_COMMAND =
  "fold_coding_session_policies_command";
export const CODING_SESSION_POLICY_FOLD_REQUEST_SCHEMA =
  "buzz-coding-session-policy-fold-request/v1";
export const CODING_SESSION_POLICY_FOLD_ADAPTER_SCHEMA =
  "buzz-coding-session-policy-fold-adapter/v1";

/** The policy in force, with the provenance a reader needs to check it. */
export type CodingSessionPolicyFoldSelected = {
  readonly eventId: string;
  readonly authorPubkey: string;
  readonly authorIsFounder: boolean;
  readonly createdAt: number;
  readonly record: CodingSessionPolicyRecord;
};

/** One record the fold refused, exactly as `bee sessions policy get` prints it. */
export type CodingSessionPolicyFoldExclusion = {
  readonly eventId: string;
  readonly authorPubkey: string;
  readonly createdAt: number;
  readonly code: string;
  readonly reason: string;
};

/**
 * One claimed authority grant the native boundary refused (REVIEW-L2 F15).
 *
 * Required rather than optional: an adapter that stopped disclosing these
 * would read exactly like one where nothing was refused, which is the fact the
 * list exists to carry.
 */
export type CodingSessionPolicyRefusedGrant = {
  readonly transitionEventId: string;
  readonly reason: string;
};

/** The native fold's whole answer. */
export type CodingSessionPolicyFoldResult = {
  readonly schema: typeof CODING_SESSION_POLICY_FOLD_ADAPTER_SCHEMA;
  readonly implementation: "buzz-core";
  readonly selected: CodingSessionPolicyFoldSelected | null;
  readonly excluded: readonly CodingSessionPolicyFoldExclusion[];
  readonly refusedGrants: readonly CodingSessionPolicyRefusedGrant[];
  readonly enforcement: string;
};

/** What the Context tab renders, in one shape with no branching left to do. */
export type CodingSessionMissionPolicyView =
  | {
      kind: "unknown";
      sentence: string;
      refused: readonly CodingSessionPolicyRefusedRow[];
    }
  /**
   * The fold selected nothing.
   *
   * REVIEW-L2 F5: this used to carry no `refused` list at all, so a channel
   * where a stranger's forged ceiling was the **only** 44245 read exactly like
   * a channel where nobody had tried — the simplest form of the attack the
   * refusal list exists to answer.
   */
  | {
      kind: "none";
      sentence: string;
      refused: readonly CodingSessionPolicyRefusedRow[];
    }
  | {
      kind: "withdrawn";
      sentence: string;
      eventId: string;
      authorPubkey: string;
      createdAt: number;
      refused: readonly CodingSessionPolicyRefusedRow[];
    }
  | {
      kind: "record";
      /** The enforcement sentence, verbatim; never re-worded by a surface. */
      sentence: string;
      eventId: string;
      authorPubkey: string;
      createdAt: number;
      facts: readonly CodingSessionPolicyFact[];
      refused: readonly CodingSessionPolicyRefusedRow[];
    };

/** One refused record as a row: who, which rule, and the fold's own sentence. */
export type CodingSessionPolicyRefusedRow = {
  eventId: string;
  /** A resolved name, or the first eight hex — never a bare 64-hex key. */
  authorLabel: string;
  code: string;
  reason: string;
};

/** How many refused records the Context tab lists before it says it stopped. */
export const MAX_CODING_SESSION_POLICY_REFUSED_ROWS = 20;

function isExclusion(
  value: unknown,
): value is CodingSessionPolicyFoldExclusion {
  return (
    hasExactFields(value, [
      ["eventId", "authorPubkey", "createdAt", "code", "reason"],
    ]) &&
    typeof value.eventId === "string" &&
    typeof value.authorPubkey === "string" &&
    Number.isSafeInteger(value.createdAt) &&
    typeof value.code === "string" &&
    typeof value.reason === "string"
  );
}

function isRefusedGrant(
  value: unknown,
): value is CodingSessionPolicyRefusedGrant {
  return (
    hasExactFields(value, [["transitionEventId", "reason"]]) &&
    typeof value.transitionEventId === "string" &&
    typeof value.reason === "string"
  );
}

/**
 * Decode the fold boundary's response.
 *
 * Exact-field in both directions, like every other native boundary here: an
 * adapter that stops disclosing `excluded` must be a loud failure, because a
 * silently missing refusal list is indistinguishable from "nothing was
 * refused" — which is the exact fact the list exists to carry.
 */
export function decodeCodingSessionPolicyFoldResult(
  value: unknown,
): CodingSessionPolicyFoldResult {
  if (
    !hasExactFields(value, [
      [
        "schema",
        "implementation",
        "selected",
        "excluded",
        "refusedGrants",
        "enforcement",
      ],
    ]) ||
    value.schema !== CODING_SESSION_POLICY_FOLD_ADAPTER_SCHEMA ||
    value.implementation !== "buzz-core" ||
    typeof value.enforcement !== "string" ||
    !Array.isArray(value.excluded) ||
    !value.excluded.every(isExclusion) ||
    !Array.isArray(value.refusedGrants) ||
    !value.refusedGrants.every(isRefusedGrant)
  ) {
    throw new Error("native session policy returned a malformed fold response");
  }
  const selected = value.selected;
  if (selected !== null) {
    if (
      !hasExactFields(selected, [
        ["eventId", "authorPubkey", "authorIsFounder", "createdAt", "record"],
      ]) ||
      typeof selected.eventId !== "string" ||
      typeof selected.authorPubkey !== "string" ||
      typeof selected.authorIsFounder !== "boolean" ||
      !Number.isSafeInteger(selected.createdAt)
    ) {
      throw new Error(
        "native session policy returned a malformed fold response",
      );
    }
  }
  return {
    schema: CODING_SESSION_POLICY_FOLD_ADAPTER_SCHEMA,
    implementation: "buzz-core",
    refusedGrants:
      value.refusedGrants as readonly CodingSessionPolicyRefusedGrant[],
    selected:
      selected === null
        ? null
        : {
            eventId: selected.eventId as string,
            authorPubkey: selected.authorPubkey as string,
            authorIsFounder: selected.authorIsFounder as boolean,
            createdAt: selected.createdAt as number,
            record: decodeCodingSessionPolicyRecord(
              (selected as { record: unknown }).record,
            ),
          },
    excluded: value.excluded as readonly CodingSessionPolicyFoldExclusion[],
    enforcement: value.enforcement,
  };
}

/**
 * The Context tab's whole answer about policy.
 *
 * `fold === null` is `unknown` — no fold has run — which is not the same fact
 * as "no policy was set", and the two get different sentences.
 */
export function codingSessionMissionPolicyView(input: {
  fold: CodingSessionPolicyFoldResult | null;
  /** Actor pubkey → display name, from the surface's own resolver. */
  resolveActorLabel?: (pubkey: string) => string | null;
  /** The umbrella's founder, so their own record reads `the founder`. */
  founderPubkey?: string | null;
}): CodingSessionMissionPolicyView {
  if (input.fold === null) {
    return {
      kind: "unknown",
      sentence: "Session policy not read yet.",
      refused: [],
    };
  }
  const refused = input.fold.excluded
    .slice(0, MAX_CODING_SESSION_POLICY_REFUSED_ROWS)
    .map((item) => ({
      eventId: item.eventId,
      authorLabel: who(item.authorPubkey, input),
      code: item.code,
      reason: item.reason,
    }));
  const selected = input.fold.selected;
  if (selected === null) {
    // "No policy set" is a claim about the wire. It is only true when nothing
    // was refused either; otherwise somebody published one and this fold
    // would not have it, and the reader is owed both halves.
    return {
      kind: "none",
      sentence:
        refused.length === 0
          ? "No policy set for this session"
          : `No policy in force · ${refused.length} refused`,
      refused,
    };
  }
  if (!selected.record.setsAnyPolicy) {
    return {
      kind: "withdrawn",
      // A withdrawal is a decision somebody made, and saying "none" here would
      // hide both the decision and the person who made it.
      sentence: `Policy withdrawn by ${who(selected.authorPubkey, input)}`,
      eventId: selected.eventId,
      authorPubkey: selected.authorPubkey,
      createdAt: selected.createdAt,
      refused,
    };
  }
  return {
    kind: "record",
    // The fold's own enforcement sentence when it supplied one, and this
    // build's otherwise — never new prose, and never softer.
    sentence:
      input.fold.enforcement.trim().length > 0
        ? input.fold.enforcement
        : CODING_SESSION_POLICY_STATED_NOT_ENFORCED,
    eventId: selected.eventId,
    authorPubkey: selected.authorPubkey,
    createdAt: selected.createdAt,
    facts: codingSessionPolicyFacts(selected.record),
    refused,
  };
}

/** How many refused rows the fold produced beyond the ones listed. */
export function codingSessionMissionPolicyRefusedOmitted(
  fold: CodingSessionPolicyFoldResult | null,
): number {
  if (fold === null) return 0;
  return Math.max(
    0,
    fold.excluded.length - MAX_CODING_SESSION_POLICY_REFUSED_ROWS,
  );
}

function who(
  pubkey: string,
  input: {
    resolveActorLabel?: (pubkey: string) => string | null;
    founderPubkey?: string | null;
  },
): string {
  const founder = input.founderPubkey?.toLowerCase() ?? null;
  if (founder !== null && founder === pubkey.toLowerCase()) {
    return "the founder";
  }
  const resolved = input.resolveActorLabel?.(pubkey)?.trim();
  // The repository's one display form. A hand-rolled `slice(0, 8)` here failed
  // `check-pubkey-truncation`, a gate the lane's §6 list never ran (F1b).
  return resolved && resolved.length > 0 ? resolved : truncatePubkey(pubkey);
}
