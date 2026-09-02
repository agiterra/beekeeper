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

/**
 * Seat tool name → C2 verb, for the calls the classifier has no rule for.
 *
 * The classifier's developer-harness rules are written for `buzz-dev-mcp`
 * names (`shell`, `read_file`, `str_replace`); a seat driven by Claude Code
 * calls the same things `Bash`, `Read`, `Edit`, `Grep`, and every one of them
 * fell through to `generic`. That is what made the 2026-09-01 live run's
 * bundle read `Relay 1 · Tool 49` over a turn that was almost entirely shell
 * and file work — a true count of a word that told the reader nothing.
 *
 * Names only, lowercased; never the call's arguments. `Write` is an `Edit`
 * because C2's verb names what the call did to the tree, not which API it
 * used, and `Glob`/`Grep` are both `Search` for the same reason.
 */
const TOOL_NAME_VERB: Readonly<Record<string, string>> = {
  bash: "Terminal",
  bashoutput: "Terminal",
  killshell: "Terminal",
  shell: "Terminal",
  terminal: "Terminal",
  read: "Read",
  notebookread: "Read",
  readfile: "Read",
  edit: "Edit",
  multiedit: "Edit",
  notebookedit: "Edit",
  write: "Edit",
  glob: "Search",
  grep: "Search",
  search: "Search",
  websearch: "Search",
};

/**
 * ACP's own tool discriminant → C2 verb, consulted after the name.
 *
 * Authoritative where the name is not: claude-agent-acp opens an edit titled
 * `Preparing file…` while the arguments are still streaming, and an MCP tool
 * carries a vendor-prefixed name no rule can match. `toolKind` is the
 * producer's own word for what the call *is*.
 */
const TOOL_KIND_VERB: Readonly<Record<string, string>> = {
  execute: "Terminal",
  read: "Read",
  edit: "Edit",
  search: "Search",
};

/** Print order; classifier-labelled leftovers follow, alphabetically. */
const VERB_ORDER = [
  "Terminal",
  "Read",
  "Edit",
  "Search",
  "Image",
  "Relay",
] as const;

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

/**
 * The C2 verb for one signed tool item.
 *
 * Order is deliberate. The classifier speaks first, because it is the only
 * thing that knows a shell command was really `bee` (`relay-op`) or that a
 * dev-MCP call edited a file. Only when it answers `generic` — "I have no rule
 * for this name" — do the seat-tool tables get a turn, and after them the
 * classifier's own label, so an unrecognised call reads `Tool` here and
 * `Ran tool` in the row the disclosure reveals. Nothing is dropped and no verb
 * is invented, so the breakdown always sums to `count`.
 */
function bundleVerb(item: CodingSessionMissionExecutionItem): string {
  const renderClass = item.descriptor?.renderClass ?? item.renderClass;
  const classified = BUNDLE_VERB[renderClass];
  if (classified !== undefined) return classified;
  if (renderClass === "generic") {
    const named = TOOL_NAME_VERB[normalizeToolName(item.toolName)];
    if (named !== undefined) return named;
    const kind = item.toolKind?.trim().toLowerCase();
    const byKind = kind ? TOOL_KIND_VERB[kind] : undefined;
    if (byKind !== undefined) return byKind;
  }
  return renderClassLabel(renderClass);
}

/** `MultiEdit` / `notebook_read` / `Buzz Dev MCP Shell` → `multiedit` … */
function normalizeToolName(toolName: string): string {
  return toolName
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "");
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
    const verb = bundleVerb(item);
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
