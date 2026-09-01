/**
 * The collapsed execution bundle (SURFACES C2, DESIGN-SPEC §3 Region C).
 *
 * A turn's signed 44225 tool items are the *how*, not the *what*. Listed inline
 * they are the loudest thing in the stream — the 2026-08-29 walk read a wall of
 * `Edited CodingSessionSurfaceHost… +1 −1 1.0s` rows above the sentence that
 * mattered. Collapsed, they become one honest line: how many events, and which
 * verbs, from the classifier the transcript already trusts.
 *
 * Two rules this module exists to keep:
 *
 * - **Reversibility.** `count` is exactly the number of items the expanded
 *   disclosure reveals; nothing is summarised away. That equality is the whole
 *   contract of a collapse and it is what the test asserts.
 * - **Never `0`.** A verb with no events is omitted, not printed as zero — the
 *   same absence-≠-zero rule the rest of this surface follows.
 */
import { renderClassLabel } from "@/features/agents/ui/agentSessionToolClassifier";
import type {
  AgentActivityRenderClass,
  TranscriptItem,
} from "@/features/agents/ui/agentSessionTypes";

/**
 * Classified render class → the verb SURFACES C2 prints. The classifier's own
 * labels ("Shell command", "File read") are diagnostic names; these are the
 * words a reader scans. Any class without a C2 verb keeps the classifier's own
 * label rather than an invented one — a call the wire did not describe well
 * enough to name reads `Tool`, which is what the expanded row calls it too.
 * Nothing is dropped, so the verbs always sum to `count`.
 */
const BUNDLE_VERB: Readonly<Partial<Record<AgentActivityRenderClass, string>>> =
  {
    shell: "Terminal",
    "file-read": "Read",
    "skill-read": "Read",
    "file-edit": "Edit",
    image: "Image",
    "relay-op": "Relay",
  };

/** Print order; classifier-labelled leftovers follow, alphabetically. */
const VERB_ORDER = ["Terminal", "Read", "Edit", "Image", "Relay"] as const;

export type CodingSessionMissionExecutionSummary = {
  /** Signed tool items in this block — equals the rows the disclosure reveals. */
  count: number;
  /** `[{ verb: "Terminal", count: 4 }, …]`, non-zero only, in print order. */
  breakdown: ReadonlyArray<{ verb: string; count: number }>;
  /** `11 execution events` / `1 execution event`. */
  label: string;
};

/** One signed tool item — the only kind of item the bundle collapses. */
type CodingSessionMissionExecutionItem = Extract<
  TranscriptItem,
  { type: "tool" }
>;

/** Is this item one of the execution events the bundle collapses? */
export function isCodingSessionMissionExecutionItem(
  item: TranscriptItem,
): item is CodingSessionMissionExecutionItem {
  return item.type === "tool";
}

/** Summarise one turn block's signed tool items for the collapsed row. */
export function summarizeCodingSessionMissionExecution(
  items: readonly TranscriptItem[],
): CodingSessionMissionExecutionSummary {
  const counts = new Map<string, number>();
  let count = 0;
  for (const item of items) {
    if (!isCodingSessionMissionExecutionItem(item)) continue;
    count += 1;
    const renderClass = item.descriptor?.renderClass ?? item.renderClass;
    const verb = BUNDLE_VERB[renderClass] ?? renderClassLabel(renderClass);
    counts.set(verb, (counts.get(verb) ?? 0) + 1);
  }
  const ordered = [...counts.entries()].sort((left, right) => {
    const leftIndex = VERB_ORDER.indexOf(
      left[0] as (typeof VERB_ORDER)[number],
    );
    const rightIndex = VERB_ORDER.indexOf(
      right[0] as (typeof VERB_ORDER)[number],
    );
    if (leftIndex !== rightIndex) {
      return (
        (leftIndex === -1 ? VERB_ORDER.length : leftIndex) -
        (rightIndex === -1 ? VERB_ORDER.length : rightIndex)
      );
    }
    return left[0].localeCompare(right[0]);
  });
  return {
    count,
    breakdown: ordered.map(([verb, verbCount]) => ({
      verb,
      count: verbCount,
    })),
    label: `${count} execution event${count === 1 ? "" : "s"}`,
  };
}
