import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import {
  isCodingSessionTaskNotificationItem,
  isCodingSessionTranscriptError,
  type CodingSessionTranscriptEntry,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import { formatTranscriptTimestampTitle } from "@/features/agents/ui/agentSessionUtils";
import { formatItemTimestamp } from "@/shared/lib/datetime";

/**
 * The transcript's vertical rhythm (SV-08) and its block times (SV-07).
 *
 * T3 Code spaces a conversation by what sits next to what, not with one gap
 * and a rule above every turn: activity rows stack tightly, prose breathes,
 * the space under a prompt is generous, and a hairline under the "Worked
 * for …" row is the only rule. This module is the one place those gaps are
 * named, as a Tailwind class (for the row) and as pixels (for the
 * virtualizer's size estimate), so the two cannot drift apart.
 */

/** What a row is, for spacing purposes. */
export type CodingSessionRowKind =
  | "prompt"
  | "prose"
  | "activity"
  | "alert"
  | "fold";

/** The rows a turn appends after its entries. */
export type CodingSessionTurnTailKind = "working" | "changed-files" | "meta";

export type CodingSessionRowGap = {
  /** A static Tailwind margin class, so the class scanner sees it. */
  className: string;
  /** The same gap in pixels at 1x, for size estimates. */
  px: number;
};

const NO_GAP: CodingSessionRowGap = { className: "", px: 0 };
const GAP_TIGHT: CodingSessionRowGap = { className: "mt-0.5", px: 2 };
const GAP_SMALL: CodingSessionRowGap = { className: "mt-1", px: 4 };
const GAP_MEDIUM: CodingSessionRowGap = { className: "mt-2", px: 8 };
const GAP_PROSE: CodingSessionRowGap = { className: "mt-3", px: 12 };
const GAP_AFTER_PROMPT: CodingSessionRowGap = { className: "mt-6", px: 24 };

/** Space between two whole turns (or standalone rows) in the list. */
export const CODING_SESSION_TURN_GAP: CodingSessionRowGap = {
  className: "gap-6",
  px: 24,
};

/** Padding the virtualized list puts under each row; equals the static gap. */
export const CODING_SESSION_VIRTUAL_ROW_PAD_CLASS = "pb-6";

export function codingSessionEntryRowKind(
  entry: CodingSessionTranscriptEntry,
): CodingSessionRowKind {
  if (entry.kind !== "item") return "activity";
  return codingSessionItemRowKind(entry.item);
}

export function codingSessionItemRowKind(
  item: TranscriptItem,
): CodingSessionRowKind {
  if (item.type === "message") {
    // A task notification is the runtime waking the agent, read as a quiet
    // activity row rather than a prompt (SV-78).
    if (isCodingSessionTaskNotificationItem(item)) return "activity";
    return item.role === "user" ? "prompt" : "prose";
  }
  if (isCodingSessionTranscriptError(item)) return "alert";
  return "activity";
}

/** The gap above `current` when `previous` sits directly above it. */
export function codingSessionRowGap(
  previous: CodingSessionRowKind | null,
  current: CodingSessionRowKind,
): CodingSessionRowGap {
  if (previous === null) return NO_GAP;
  if (previous === "prompt") return GAP_AFTER_PROMPT;
  // The fold row carries its own hairline and padding underneath.
  if (previous === "fold" || current === "fold") return GAP_MEDIUM;
  if (previous === "activity" && current === "activity") return GAP_TIGHT;
  if (previous === "prose" && current === "prose") return GAP_PROSE;
  return GAP_MEDIUM;
}

/** The gap above one of the rows a turn adds after its entries. */
export function codingSessionTurnTailGap(
  previous: CodingSessionRowKind | null,
  tail: CodingSessionTurnTailKind,
): CodingSessionRowGap {
  if (previous === null) return NO_GAP;
  if (tail === "changed-files") return GAP_MEDIUM;
  return GAP_SMALL;
}

/**
 * A block's time, for the hover line beside it: the clock time today, then
 * "Yesterday at …", a weekday, a date — the same labels the rest of the app
 * uses for a conversation item. `null` for a timestamp that does not parse:
 * no time is said rather than a wrong one.
 */
export function formatCodingSessionBlockTime(
  timestamp: string | null | undefined,
  nowMs: number = Date.now(),
): { label: string; title: string | undefined } | null {
  if (!timestamp) return null;
  const ms = Date.parse(timestamp);
  if (!Number.isFinite(ms)) return null;
  return {
    label: formatItemTimestamp(ms / 1_000, {
      withTime: true,
      nowSeconds: nowMs / 1_000,
    }),
    title: formatTranscriptTimestampTitle(timestamp),
  };
}
