/**
 * Typed 44244 transactions, projected into stream rows.
 *
 * The causality plane. Until now the signed assignment → report → verdict →
 * acknowledgement chain lived only as a list inside one pinned card above the
 * stream, so the order a person actually reads — chronology — did not carry
 * the handoff at all. These rows put each signed transaction where it happened,
 * with the two parties named in a sentence.
 *
 * Nothing here is inferred. Every field is copied from
 * {@link CodingSessionMissionTransactionInput}, which Lane D projects from the
 * Rust fold; the only derivations are display ones (a monogram, an identity
 * accent, a local time label) and the weight, which follows the frozen rule in
 * the batch spec. `unseated` and `delivery` are disclosure the row must not
 * drop: a report whose author holds no seat, and a wake that never reached the
 * lead, are exactly the facts a tidier row would hide.
 */
import type {
  CodingSessionMissionTransactionInput,
  CodingSessionMissionTransactionType,
  CodingSessionTeamWakeDelivery,
} from "./codingSessionMissionContracts";
import type { CodingSessionMissionDensity } from "./codingSessionMissionDensity";
import {
  codingSessionParticipantAccent,
  type CodingSessionParticipantAccent,
} from "./codingSessionParticipantAccent";
import type { CodingSessionMissionRowWeight } from "./codingSessionMissionRowGrammar";
import { CODING_SESSION_UNKNOWN_ACTOR } from "./codingSessionTurnByline";

/** One party on a transaction row: a name, a monogram, and its identity accent. */
export type CodingSessionMissionRowParty = {
  pubkey: string;
  label: string;
  /** First grapheme of the label. Decorative — always `aria-hidden`. */
  monogram: string;
  accent: CodingSessionParticipantAccent;
};

/**
 * How a pubkey becomes a name. The finalizer supplies this from
 * `umbrella.executions[].activeGeneration.agentRef` plus the workspace actor
 * resolver; an unresolvable pubkey returns a null label and the row says
 * {@link CODING_SESSION_UNKNOWN_ACTOR} rather than printing a key.
 */
export type CodingSessionMissionActorResolver = (pubkey: string) => {
  label: string | null;
  executionKey: string | null;
};

/** One signed transaction, ready to render. */
export type CodingSessionMissionTransactionRow = {
  key: string;
  type: CodingSessionMissionTransactionType;
  weight: CodingSessionMissionRowWeight;
  /** `critical` only where a fact is unanswered; `caution` where one is owed. */
  tone: "critical" | "caution" | null;
  actor: CodingSessionMissionRowParty;
  counterparty: CodingSessionMissionRowParty | null;
  /** Frozen title copy — see the batch spec's transaction-row table. */
  title: string;
  /** The signed summary, verbatim. */
  body: string;
  meta: {
    timeLabel: string;
    sourceEventId: string;
  };
  /** Sentence a screen reader hears in place of the arrow glyph. */
  accessibleLabel: string;
  decision: string | null;
  requiredAction: string | null;
  delivery: CodingSessionTeamWakeDelivery | null;
  unseated: boolean;
  fileCount: number | null;
  testCount: number | null;
  /** Unix seconds from the signed event; display ordering only. */
  createdAt: number;
  /** Trace alone shows the full signed source id under the existing disclosure. */
  showSignedSource: boolean;
};

const TYPE_WORD: Readonly<Record<CodingSessionMissionTransactionType, string>> =
  {
    assignment: "assignment",
    report: "report",
    refutation: "refutation",
    disposition: "verdict",
    acknowledgement: "acknowledgement",
    "mission.completed": "mission completed",
    "mission.blocked": "mission blocked",
    note: "note",
    // B1c's two decision verbs read as a ruling asked for and a ruling given,
    // which is what a person watching the team is actually waiting on. The
    // wire words stay `decision.request` / `decision.answer`.
    "decision.request": "ruling asked",
    "decision.answer": "ruling given",
  };

/**
 * The row's word for one type. A type this build has never heard of says its
 * own wire word rather than reading as `undefined` to a screen reader.
 */
function typeWord(type: CodingSessionMissionTransactionType): string {
  return TYPE_WORD[type] ?? type;
}

function monogramOf(label: string): string {
  const trimmed = label.trim();
  if (trimmed.length === 0) return "?";
  const segmenter =
    typeof Intl !== "undefined" && "Segmenter" in Intl
      ? new Intl.Segmenter(undefined, { granularity: "grapheme" })
      : null;
  const first = segmenter
    ? (segmenter.segment(trimmed)[Symbol.iterator]().next().value?.segment ??
      trimmed)
    : (Array.from(trimmed)[0] ?? trimmed);
  return first.toLocaleUpperCase();
}

function party(input: {
  pubkey: string;
  founderPubkey: string | null;
  resolveActor: CodingSessionMissionActorResolver;
}): CodingSessionMissionRowParty {
  const resolved = input.resolveActor(input.pubkey);
  const isFounder =
    input.founderPubkey !== null &&
    input.founderPubkey.toLowerCase() === input.pubkey.toLowerCase();
  const label =
    resolved.label ?? (isFounder ? "You" : CODING_SESSION_UNKNOWN_ACTOR);
  return {
    pubkey: input.pubkey,
    label,
    monogram: monogramOf(label),
    accent: codingSessionParticipantAccent(
      resolved.executionKey ?? input.pubkey,
    ),
  };
}

/**
 * The frozen title word for one type.
 *
 * A `switch` rather than a conditional chain: a chain's final `else` silently
 * titled every unlisted type `Verdict: …`, which is how B1c's three verbs came
 * out of Mission wearing a verdict's words. The `never` binding makes the next
 * new verb a compile error here, and the string fallback means an unknown one
 * still says its own name instead of borrowing someone else's.
 */
function titleSuffixFor(
  type: CodingSessionMissionTransactionType,
  decision: string | null,
): string {
  switch (type) {
    case "assignment":
      return "Assignment";
    case "report":
      return "Report";
    case "refutation":
      return "Refutation";
    case "disposition":
      return `Verdict: ${decision ?? "decision not reported"}`;
    case "acknowledgement":
      return "Acknowledgement";
    case "note":
      return "Note";
    case "decision.request":
      return "Ruling asked";
    case "decision.answer":
      return "Ruling given";
    case "mission.completed":
      return "Mission completed";
    case "mission.blocked":
      return "Mission blocked";
    default: {
      const unlisted: never = type;
      return String(unlisted);
    }
  }
}

function titleFor(input: {
  type: CodingSessionMissionTransactionType;
  actorLabel: string;
  counterpartyLabel: string | null;
  decision: string | null;
}): string {
  if (input.type === "mission.completed") {
    return `Mission completed · ${input.actorLabel}`;
  }
  if (input.type === "mission.blocked") {
    return `Mission blocked · ${input.actorLabel}`;
  }
  const suffix = titleSuffixFor(input.type, input.decision);
  const parties =
    input.counterpartyLabel === null
      ? input.actorLabel
      : `${input.actorLabel} → ${input.counterpartyLabel}`;
  return `${parties} · ${suffix}`;
}

function weightFor(input: {
  type: CodingSessionMissionTransactionType;
  requiredAction: string | null;
  delivery: CodingSessionTeamWakeDelivery | null;
}): {
  weight: CodingSessionMissionRowWeight;
  tone: "critical" | "caution" | null;
} {
  if (input.type === "mission.blocked" || input.delivery?.kind === "failed") {
    return { weight: "attention", tone: "critical" };
  }
  if (input.type === "refutation" || input.requiredAction !== null) {
    return { weight: "attention", tone: "caution" };
  }
  // B1c's three verbs are standard weight. A `decision.request` is the one
  // that tempts otherwise, but this builder cannot see whether its answer has
  // landed, and a request that reads `attention` forever after it was answered
  // is a badge that lies. Weighting an *open* ruling is a fold fact, not a row
  // fact, so it waits for the fold to report one.
  return { weight: "standard", tone: null };
}

function timeLabel(createdAt: number): string {
  const date = new Date(createdAt * 1_000);
  return Number.isFinite(date.getTime())
    ? date.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })
    : "time not reported";
}

/**
 * Project signed transactions into stream rows.
 *
 * Rows come back in the caller's order; the stream model does the chronological
 * merge and the bounding. Every transaction produces exactly one row — Brief
 * never drops typed mission state.
 */
export function buildCodingSessionMissionTransactionRows(input: {
  transactions: readonly CodingSessionMissionTransactionInput[];
  resolveActor: CodingSessionMissionActorResolver;
  founderPubkey: string | null;
  deliveries?: readonly CodingSessionTeamWakeDelivery[];
  density: CodingSessionMissionDensity;
}): CodingSessionMissionTransactionRow[] {
  const deliveriesBySource = new Map<string, CodingSessionTeamWakeDelivery>();
  for (const delivery of input.deliveries ?? []) {
    const existing = deliveriesBySource.get(delivery.sourceEventId);
    if (
      existing === undefined ||
      (delivery.observedAtMs ?? 0) >= (existing.observedAtMs ?? 0)
    ) {
      deliveriesBySource.set(delivery.sourceEventId, delivery);
    }
  }
  return input.transactions.map((transaction) => {
    const actor = party({
      pubkey: transaction.authorPubkey,
      founderPubkey: input.founderPubkey,
      resolveActor: input.resolveActor,
    });
    const counterparty =
      transaction.counterpartyPubkey === null
        ? null
        : party({
            pubkey: transaction.counterpartyPubkey,
            founderPubkey: input.founderPubkey,
            resolveActor: input.resolveActor,
          });
    const delivery = deliveriesBySource.get(transaction.sourceEventId) ?? null;
    const { weight, tone } = weightFor({
      type: transaction.type,
      requiredAction: transaction.requiredAction,
      delivery,
    });
    return {
      key: `transaction:${transaction.sourceEventId}`,
      type: transaction.type,
      weight,
      tone,
      actor,
      counterparty,
      title: titleFor({
        type: transaction.type,
        actorLabel: actor.label,
        counterpartyLabel: counterparty?.label ?? null,
        decision: transaction.decision,
      }),
      body: transaction.summary,
      meta: {
        timeLabel: timeLabel(transaction.createdAt),
        sourceEventId: transaction.sourceEventId,
      },
      accessibleLabel:
        counterparty === null
          ? `${actor.label}: ${typeWord(transaction.type)}`
          : `${actor.label} to ${counterparty.label}: ${typeWord(transaction.type)}`,
      decision: transaction.decision,
      requiredAction: transaction.requiredAction,
      delivery,
      unseated: transaction.unseated,
      fileCount: transaction.fileCount,
      testCount: transaction.testCount,
      createdAt: transaction.createdAt,
      showSignedSource: input.density === "trace",
    } satisfies CodingSessionMissionTransactionRow;
  });
}
