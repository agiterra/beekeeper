/**
 * SV-29 "Edit from here": rewind an execution to before one of its turns.
 *
 * The wire (WIRE-C3b): a 44221 `session.rewind` names the exact current
 * generation N, the turn's 44231 checkpoint, and whether the working tree is
 * kept or restored. The provider detaches N — its native session is left as
 * it was, never truncated — and opens N+1 seeded only from the signed record
 * up to the cut, answering with a `resumed` / `resumed_without_context` 44224
 * that carries a `rewind` field, or a `failed` one with a code.
 *
 * Everything here is pure: what may be offered for a prompt, how a published
 * rewind ended, and the join that names who rewound which turns. The words
 * live in `codingSessionRewindRows.ts`; the dialog in
 * `ui/CodingSessionRewindDialog.tsx`.
 */
import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_CODING_SESSION_LIFECYCLE_COMMAND } from "@/shared/constants/kinds";
import {
  hasStrictLifecycleCommandJson,
  hasStrictLifecycleCommandValues,
} from "@/shared/coordination/sessionCoordinationStrictJson";
import type {
  SessionRewindFilesOutcome,
  SessionRewindReceiptFacts,
} from "@/shared/coordination/sessionCoordinationRewind";
import type { CodingSessionCheckpointEntry } from "./codingSessionCheckpoints";
import {
  buildCodingSessionTargetKey,
  type CodingSessionCommandTarget,
} from "./codingSessionCommand";
import { parseCodingSessionLifecycleReceipt } from "./codingSessionIngressPayloads";
import type { CodingSessionLifecycleResolution } from "./codingSessionTrustedIngress";

export {
  CODING_SESSION_REWOUND_STATUS,
  CODING_SESSION_REWOUND_TEXT,
  CODING_SESSION_REWOUND_TITLE,
  codingSessionRewoundStatusRow,
  isCodingSessionRewoundRow,
} from "./codingSessionRewindRows";
export type {
  SessionRewindFilesOutcome,
  SessionRewindReceiptFacts,
} from "@/shared/coordination/sessionCoordinationRewind";

/** The two choices the dialog offers. */
export type CodingSessionRewindFiles = "keep" | "restore";

/** The provider's refusal codes a rewind adds beside `SESSION_BUSY`. */
export const CODING_SESSION_REWIND_CODES = [
  "SESSION_BUSY",
  "TREE_BUSY",
  "CHECKPOINT_UNAVAILABLE",
  "CHECKPOINT_NOT_THIS_EXECUTION",
  "NOT_RESTORABLE",
  "REWIND_NOT_RESTARTED",
] as const;

/** Why one choice is not offered, in the order the dialog checks them. */
export type CodingSessionRewindBlock =
  | "outside-generation"
  | "session-closed"
  | "authority-unresolved"
  | "not-controller"
  | "turn-running"
  | "no-checkpoint"
  | "provider-cannot-rewind"
  | "checkpoint-not-restorable"
  | "no-git";

export type CodingSessionRewindChoice =
  | { enabled: true }
  | { enabled: false; block: CodingSessionRewindBlock };

export type CodingSessionRewindAvailability = {
  chat: CodingSessionRewindChoice;
  files: CodingSessionRewindChoice;
  /** The 44231 a published rewind names; null when there is none to name. */
  checkpointEventId: string | null;
};

const ON: CodingSessionRewindChoice = { enabled: true };
const off = (block: CodingSessionRewindBlock): CodingSessionRewindChoice => ({
  enabled: false,
  block,
});

/**
 * What the dialog may offer for one prompt.
 *
 * Every gate the provider applies is mirrored, never widened: a rewind is
 * addressed to the current generation (an older one is `STALE_GENERATION`),
 * needs restart authority, is `SESSION_BUSY` while a turn runs, and names a
 * turn checkpoint the provider marked `restorable`. `restorable` is about the
 * conversation only (ledger 371); restoring files additionally needs
 * `git.baseTree`, read from the git facts and never inferred from
 * `restorable`.
 *
 * `restorable: false` refuses both choices, as the provider does
 * (`NOT_RESTORABLE`). With git trees it came from a build that cannot rewind
 * at all (C2 hard-coded it false); without a `baseTree` it may also be a
 * rewind-capable build that tied `restorable` to the baseline, so the reason
 * names the checkpoint rather than the build. A `restorable` checkpoint with
 * no `git`, or no `baseTree` (taken outside a repository, or its baseline
 * was not captured in time), offers the chat only.
 */
export function resolveCodingSessionRewindAvailability(input: {
  checkpoint: CodingSessionCheckpointEntry | null;
  turnRunning: boolean;
  /** `null` while authority is still being resolved. */
  mayControl: boolean | null;
  inCurrentGeneration: boolean;
  /** A closed session refuses every lifecycle command (`SESSION_CLOSED`). */
  sessionClosed?: boolean;
}): CodingSessionRewindAvailability {
  const checkpoint =
    input.checkpoint?.payload.reason === "turn" ? input.checkpoint : null;
  const checkpointEventId = checkpoint?.eventId ?? null;
  const gate: CodingSessionRewindBlock | null = !input.inCurrentGeneration
    ? "outside-generation"
    : input.sessionClosed
      ? "session-closed"
      : input.mayControl === null
        ? "authority-unresolved"
        : !input.mayControl
          ? "not-controller"
          : input.turnRunning
            ? "turn-running"
            : checkpoint === null
              ? "no-checkpoint"
              : null;
  if (gate !== null) {
    return { chat: off(gate), files: off(gate), checkpointEventId };
  }
  const payload = (checkpoint as CodingSessionCheckpointEntry).payload;
  const baseTree = payload.git?.baseTree ?? null;
  if (!payload.restorable) {
    const block = off(
      baseTree !== null
        ? "provider-cannot-rewind"
        : "checkpoint-not-restorable",
    );
    return { chat: block, files: block, checkpointEventId };
  }
  const files = baseTree === null ? off("no-git") : ON;
  return { chat: ON, files, checkpointEventId };
}

/** How one published rewind ended, as far as the signed facts say. */
export type CodingSessionRewindOutcome =
  | { kind: "pending" }
  /** Two disagreeing answers from the provider: nothing is claimed. */
  | { kind: "conflict" }
  /** Refused before anything changed (steps 1–5): no `rewind` on the receipt. */
  | { kind: "refused"; code: string; message: string }
  /**
   * N+1 is open. `memory` is what it started from: the record up to the cut
   * (`seeded`), or nothing at all (`none`, a `resumed_without_context`).
   */
  | {
      kind: "rewound";
      target: CodingSessionCommandTarget;
      memory: "seeded" | "none";
      /** Null while the receipt's `rewind` has not been read in this view. */
      rewind: SessionRewindReceiptFacts | null;
      /** The provider's own sentence for the loss, when `memory` is none. */
      message: string | null;
    }
  /**
   * The cut was made — files possibly restored — but N+1 did not open, so N
   * was reopened and still remembers the turns the rewind meant to drop.
   */
  | {
      kind: "not-restarted";
      files: SessionRewindFilesOutcome | null;
      message: string;
    };

/** Fold a rewind's lifecycle resolution (and its receipt's `rewind`). */
export function foldCodingSessionRewindOutcome(input: {
  lifecycle: CodingSessionLifecycleResolution | null;
  /** The success receipt's `rewind`, read off its retained raw event. */
  rewind: SessionRewindReceiptFacts | null;
  /** `rewind` on a failed receipt (`REWIND_NOT_RESTARTED` carries it). */
  failedRewind?: SessionRewindReceiptFacts | null;
}): CodingSessionRewindOutcome {
  const lifecycle = input.lifecycle;
  if (lifecycle === null || lifecycle.state === "pending") {
    return { kind: "pending" };
  }
  if (lifecycle.state === "conflict") return { kind: "conflict" };
  if (lifecycle.state === "failed") {
    if (lifecycle.error.code === "REWIND_NOT_RESTARTED") {
      return {
        kind: "not-restarted",
        files: input.failedRewind?.files ?? null,
        message: lifecycle.error.message,
      };
    }
    return {
      kind: "refused",
      code: lifecycle.error.code,
      message: lifecycle.error.message,
    };
  }
  if (lifecycle.state === "resumed-without-context") {
    return {
      kind: "rewound",
      target: lifecycle.target,
      memory: "none",
      rewind: input.rewind,
      message: lifecycle.error.message,
    };
  }
  // A create-shaped answer is not an answer a rewind can have.
  if (
    lifecycle.state === "created-with-failed-initial-turn" ||
    lifecycle.state === "awaiting-metadata-after-failed-initial-turn"
  ) {
    return { kind: "conflict" };
  }
  return {
    kind: "rewound",
    target: lifecycle.target,
    memory: "seeded",
    rewind: input.rewind,
    message: null,
  };
}

/**
 * The `rewind` of this command's receipt among a generation's retained raw
 * events (the store keeps every receipt that names a session), or null.
 */
export function findCodingSessionRewindReceipt(
  events: readonly RelayEvent[],
  commandId: string,
  providerAuthorityPubkey: string,
): SessionRewindReceiptFacts | null {
  for (const event of events) {
    if (event.pubkey !== providerAuthorityPubkey) continue;
    const receipt = parseCodingSessionLifecycleReceipt(event.content);
    if (receipt?.commandId === commandId && receipt.rewind) {
      return receipt.rewind;
    }
  }
  return null;
}

/** One signed rewind joined to the generation it minted. */
export type CodingSessionRewindRecord = {
  commandEventId: string;
  commandId: string;
  /** Who signed the 44221: the person (or seat) that rewound. */
  signerPubkey: string;
  requestedFiles: CodingSessionRewindFiles;
  rewind: SessionRewindReceiptFacts;
  memory: "seeded" | "none";
};

function sameTarget(
  left: CodingSessionCommandTarget,
  right: unknown,
): right is CodingSessionCommandTarget {
  if (typeof right !== "object" || right === null) return false;
  const value = right as Record<string, unknown>;
  return (
    value.driver === left.driver &&
    value.instanceId === left.instanceId &&
    value.sessionId === left.sessionId &&
    value.generation === left.generation
  );
}

/**
 * The rewind that minted `target` (generation N+1), from signature-verified
 * 44221 commands and 44224 receipts, or null when the facts do not establish
 * exactly one.
 *
 * Trust lives in the join, as it does for creates: the command names the
 * provider it addressed and generation N of the same execution; only that
 * provider's own success receipt, naming N+1 and a `rewind` cut at the same
 * checkpoint with `previousGeneration` N, joins it. Two candidates for one
 * generation prove nothing, so none is returned.
 */
export function joinCodingSessionRewindRecord(input: {
  channelId: string;
  providerAuthorityPubkey: string;
  target: CodingSessionCommandTarget;
  commands: readonly RelayEvent[];
  receipts: readonly RelayEvent[];
}): CodingSessionRewindRecord | null {
  const { target, providerAuthorityPubkey } = input;
  if (target.generation < 2) return null;
  const previous = { ...target, generation: target.generation - 1 };
  const candidates: CodingSessionRewindRecord[] = [];
  const seen = new Set<string>();
  for (const event of input.commands) {
    if (seen.has(event.id)) continue;
    seen.add(event.id);
    if (event.kind !== KIND_CODING_SESSION_LIFECYCLE_COMMAND) continue;
    if (
      !event.tags.some((tag) => tag[0] === "h" && tag[1] === input.channelId)
    ) {
      continue;
    }
    let content: unknown;
    try {
      content = JSON.parse(event.content);
    } catch {
      continue;
    }
    if (
      !hasStrictLifecycleCommandJson(event.content, content) ||
      !hasStrictLifecycleCommandValues(content)
    ) {
      continue;
    }
    const action = content.action as Record<string, unknown>;
    const commandId = content.commandId as string;
    if (
      action.type !== "session.rewind" ||
      action.providerAuthorityPubkey !== providerAuthorityPubkey ||
      !sameTarget(previous, action.session) ||
      !event.tags.some(
        (tag) => tag[0] === "csl-command" && tag[1] === commandId,
      )
    ) {
      continue;
    }
    const answers = input.receipts.flatMap((receiptEvent) => {
      if (receiptEvent.pubkey !== providerAuthorityPubkey) return [];
      const receipt = parseCodingSessionLifecycleReceipt(receiptEvent.content);
      if (
        !receipt ||
        receipt.commandId !== commandId ||
        (receipt.status !== "resumed" &&
          receipt.status !== "resumed_without_context") ||
        !sameTarget(target, receipt.session) ||
        !receipt.rewind ||
        receipt.rewind.checkpoint !== action.checkpoint ||
        receipt.rewind.previousGeneration !== previous.generation
      ) {
        return [];
      }
      return [receipt];
    });
    const distinct = new Set(answers.map((answer) => JSON.stringify(answer)));
    const [answer] = answers;
    if (distinct.size !== 1 || !answer?.rewind) continue;
    candidates.push({
      commandEventId: event.id,
      commandId,
      signerPubkey: event.pubkey,
      requestedFiles: action.files as CodingSessionRewindFiles,
      rewind: answer.rewind,
      memory: answer.status === "resumed" ? "seeded" : "none",
    });
  }
  return candidates.length === 1 ? (candidates[0] ?? null) : null;
}

/** One earlier generation's projected transcript, as the view holds it. */
export type CodingSessionRewindGenerationTranscript = {
  generation: number;
  items: readonly (TranscriptItem & { sourceEventSeq?: number })[];
};

/**
 * How many turns a rewind dropped, counted from the record this view holds,
 * or null when a generation inside the cut is not held — a count over
 * missing history would be a guess presented as a number.
 *
 * A turn is a prompt that opened one: a steered prompt joined a turn already
 * running, so it is not counted again.
 */
export function countCodingSessionRewoundTurns(input: {
  rewind: Pick<
    SessionRewindReceiptFacts,
    "cutGeneration" | "cutAfterSeq" | "previousGeneration"
  >;
  generations: readonly CodingSessionRewindGenerationTranscript[];
}): number | null {
  const { cutGeneration, cutAfterSeq, previousGeneration } = input.rewind;
  if (cutGeneration > previousGeneration) return null;
  const byGeneration = new Map(
    input.generations.map((entry) => [entry.generation, entry.items]),
  );
  let count = 0;
  for (
    let generation = cutGeneration;
    generation <= previousGeneration;
    generation += 1
  ) {
    const items = byGeneration.get(generation);
    if (!items) return null;
    for (const item of items) {
      if (item.type !== "message" || item.role !== "user" || item.steered) {
        continue;
      }
      if (generation === cutGeneration) {
        if (typeof item.sourceEventSeq !== "number") return null;
        if (item.sourceEventSeq <= cutAfterSeq) continue;
      }
      count += 1;
    }
  }
  return count;
}

/** The composer's draft-recovery request that puts the edited prompt back. */
export function codingSessionRewindPrefill(input: {
  commandId: string;
  channelId: string;
  target: CodingSessionCommandTarget;
  text: string;
}): {
  id: string;
  channelId: string;
  targetKey: string;
  text: string;
  attachmentCount: number;
} {
  return {
    id: `rewind-prefill:${input.commandId}`,
    channelId: input.channelId,
    targetKey: buildCodingSessionTargetKey(input.target),
    text: input.text,
    attachmentCount: 0,
  };
}
