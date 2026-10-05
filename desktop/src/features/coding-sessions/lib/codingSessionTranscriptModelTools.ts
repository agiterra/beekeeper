import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import { classifyTool } from "@/features/agents/ui/agentSessionToolClassifier";
import { getToolString } from "@/features/agents/ui/agentSessionUtils";
import {
  buildCodingSessionSubagentSpawn,
  type CodingSessionSubagentPartition,
  type CodingSessionSubagentSpawn,
  formatCodingSessionSubagentGroupLabel,
  isCodingSessionSubagentCall,
} from "@/features/coding-sessions/lib/codingSessionSubagents";
import { deriveCodingSessionTaskModel } from "@/features/coding-sessions/lib/codingSessionTaskModel";
import {
  isCompletedSuccessfulTool,
  isErrorItem,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModelPredicates";
import type {
  CodingSessionTranscriptEntry,
  CodingSessionTranscriptToolItem,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModelTypes";

/*
 * Turning a turn's visible items into entries: plan snapshots coalesce, Task
 * spawns gather, and runs of settled tool calls become one sentence row.
 * Split out of `codingSessionTranscriptModel.ts` for the 1000-line ceiling.
 */

/** The fewest consecutive settled tool calls that read as one row. */
const TOOL_GROUP_MIN = 2;

export type GroupAdjacentToolsOptions = {
  /**
   * The turn completed and will fold (not live, not failed, not stopped).
   * Then a failed call before the answer joins its neighbours' group, as in
   * T3's work log, where a failed command sits inside "Ran 3 commands" — it
   * is a step on the way to the answer, which the fold names. A failed call
   * after the answer, or in a turn that never answered, stays its own row.
   */
  foldsSettledWork?: boolean;
};

export function groupAdjacentTools(
  items: TranscriptItem[],
  subagents: CodingSessionSubagentPartition,
  options: GroupAdjacentToolsOptions = {},
): CodingSessionTranscriptEntry[] {
  const narrativeItems = coalescePlanSnapshots(items);
  const answerIndex = options.foldsSettledWork
    ? findAnswerIndex(narrativeItems)
    : -1;
  const entries: CodingSessionTranscriptEntry[] = [];
  const isSpawn = (candidate: TranscriptItem | undefined) =>
    candidate !== undefined &&
    isCodingSessionSubagentCall(candidate, subagents);
  const isGroupable = (
    candidate: TranscriptItem | undefined,
    position: number,
  ) =>
    candidate !== undefined &&
    candidate.type === "tool" &&
    (isCompletedSuccessfulTool(candidate) ||
      (position < answerIndex && isFailedToolCall(candidate))) &&
    !isSpawn(candidate) &&
    deriveCodingSessionTaskModel([candidate]) === null;

  for (let index = 0; index < narrativeItems.length; index += 1) {
    const item = narrativeItems[index];
    if (item.type === "tool" && isSpawn(item)) {
      const spawns: CodingSessionSubagentSpawn[] = [];
      let cursor = index;
      while (cursor < narrativeItems.length) {
        const call = narrativeItems[cursor];
        if (call?.type !== "tool" || !isSpawn(call)) break;
        spawns.push(buildCodingSessionSubagentSpawn(call, subagents));
        cursor += 1;
      }
      entries.push({
        kind: "subagents",
        id: `subagents:${item.id}`,
        label: formatCodingSessionSubagentGroupLabel(spawns),
        spawns,
      });
      index = cursor - 1;
      continue;
    }
    if (!isGroupable(item, index) || item.type !== "tool") {
      entries.push({ kind: "item", item });
      continue;
    }

    const tools: CodingSessionTranscriptToolItem[] = [item];
    let cursor = index + 1;
    while (cursor < narrativeItems.length) {
      const candidate = narrativeItems[cursor];
      if (!isGroupable(candidate, cursor) || candidate?.type !== "tool") {
        break;
      }
      tools.push(candidate);
      cursor += 1;
    }

    if (tools.length >= TOOL_GROUP_MIN) {
      const failedCount = tools.filter(isFailedToolCall).length;
      const summary = summarizeCodingSessionTools(tools);
      entries.push({
        kind: "tool-group",
        // Keyed by the first call, so the row stays mounted (and keeps its
        // open state) while a live run grows behind it.
        id: `tools:${item.id}`,
        // A failed step inside the group is named on the row itself, so a
        // closed group never reads as all-green (honesty; the fold row
        // names it too).
        label: failedCount > 0 ? `${summary} · ${failedCount} failed` : summary,
        items: tools,
        failedCount,
      });
    } else {
      entries.push({ kind: "item", item });
    }
    index = cursor - 1;
  }

  return entries;
}

function isFailedToolCall(item: TranscriptItem): boolean {
  return item.type === "tool" && isErrorItem(item);
}

/**
 * The turn's answer: its last assistant prose that says something and is not
 * a Turn result body — the same entry `deriveCodingSessionTurnFold` keeps.
 */
function findAnswerIndex(items: readonly TranscriptItem[]): number {
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index];
    if (
      item.type === "message" &&
      item.role === "assistant" &&
      item.text.trim() !== "" &&
      !item.id.endsWith(":assistant-result")
    ) {
      return index;
    }
  }
  return -1;
}

/** Keep a plan's first narrative position while replacing it with its latest snapshot. */
function coalescePlanSnapshots(items: TranscriptItem[]): TranscriptItem[] {
  const planIndexes = items.flatMap((item, index) =>
    isRenderablePlanSnapshot(item) ? [index] : [],
  );
  if (planIndexes.length < 2) return items;

  const firstPlanIndex = planIndexes[0];
  const latestPlan = items[planIndexes.at(-1) ?? firstPlanIndex];
  const planIndexSet = new Set(planIndexes);
  return items.flatMap((item, index) => {
    if (index === firstPlanIndex) return [latestPlan];
    return planIndexSet.has(index) ? [] : [item];
  });
}

function isRenderablePlanSnapshot(item: TranscriptItem): boolean {
  if (deriveCodingSessionTaskModel([item]) === null) return false;
  return item.type === "plan" || isCompletedSuccessfulTool(item);
}

type ToolAction =
  | "read"
  | "edit"
  | "command"
  | "code-search"
  | "web-search"
  | "other";

/**
 * T3 Code's work-log labels (`packages/client-runtime/src/work-log/
 * presentation.ts`, `toolGroupActionLabel`), word for word.
 */
const TOOL_ACTION_LABELS: Record<ToolAction, (count: number) => string> = {
  read: (count) => `Read ${count} ${count === 1 ? "file" : "files"}`,
  edit: (count) => `Changed ${count} ${count === 1 ? "file" : "files"}`,
  command: (count) => `Ran ${count} ${count === 1 ? "command" : "commands"}`,
  "code-search": (count) =>
    `Searched code ${count} ${count === 1 ? "time" : "times"}`,
  "web-search": (count) =>
    `Searched the web ${count} ${count === 1 ? "time" : "times"}`,
  other: (count) => `Used ${count} ${count === 1 ? "tool" : "tools"}`,
};

/**
 * Which clauses win the sentence's two places, as T3's
 * `summaryActionPriority`: what changed the world first (commands and edits),
 * then reads and searches, then anything else.
 */
const TOOL_ACTION_PRIORITY: Record<ToolAction, number> = {
  command: 0,
  edit: 0,
  read: 1,
  "code-search": 1,
  "web-search": 1,
  other: 2,
};

/** The most action clauses a sentence names before "performed N other actions". */
const TOOL_SUMMARY_MAX_CLAUSES = 2;

/**
 * One sentence for a run of tool calls, in T3 Code's words
 * (`summarizeToolGroup`): "Changed 2 files and ran 5 commands", and with a
 * third kind present "Changed 2 files, ran 5 commands, and performed 1 other
 * action". At most two kinds are named — commands and edits first, then reads
 * and searches, then other tools — in the order each first appears; every
 * call they leave out is still counted in the remainder, so the sentence
 * never claims less work than there was.
 *
 * One deliberate difference from T3: reads, like edits, count each file once
 * — three reads of one file read one file. A call that names no file counts
 * as its own, so the number never claims fewer files than the calls could
 * have touched. The remainder counts calls.
 */
export function summarizeCodingSessionTools(
  tools: readonly CodingSessionTranscriptToolItem[],
): string {
  const groups = new Map<ToolAction, { calls: number; keys: Set<string> }>();
  for (const tool of tools) {
    const action = classifyToolAction(tool);
    const key =
      action === "read" || action === "edit"
        ? (toolFileKey(tool) ?? `call:${tool.id}`)
        : `call:${tool.id}`;
    const group = groups.get(action) ?? { calls: 0, keys: new Set<string>() };
    group.calls += 1;
    group.keys.add(key);
    groups.set(action, group);
  }
  const ranked = [...groups].map(([action, group], index) => ({
    action,
    group,
    index,
  }));
  const selected = [...ranked]
    .sort(
      (a, b) =>
        TOOL_ACTION_PRIORITY[a.action] - TOOL_ACTION_PRIORITY[b.action] ||
        a.index - b.index,
    )
    .slice(0, TOOL_SUMMARY_MAX_CLAUSES)
    .sort((a, b) => a.index - b.index);
  const labels = selected.map(({ action, group }) =>
    TOOL_ACTION_LABELS[action](group.keys.size),
  );
  const remaining =
    tools.length -
    selected.reduce((count, { group }) => count + group.calls, 0);
  if (remaining > 0) {
    labels.push(
      `Performed ${remaining} other ${remaining === 1 ? "action" : "actions"}`,
    );
  }
  return joinToolSummaryClauses(labels);
}

/** "A", "A and b", "A, b, and c" — T3's sentence casing and serial comma. */
function joinToolSummaryClauses(labels: readonly string[]): string {
  const clauses = labels.map((label, index) =>
    index === 0 ? label : label.charAt(0).toLowerCase() + label.slice(1),
  );
  if (clauses.length < 3) return clauses.join(" and ");
  return `${clauses.slice(0, -1).join(", ")}, and ${clauses.at(-1)}`;
}

/**
 * What kind of work a call was, after T3's `classifyToolActivity`
 * (`packages/shared/src/toolActivity.ts`): ACP's own discriminant first, then
 * the call's descriptor, then the provider's bare tool name. Claude-agent-acp
 * titles a Bash call with its command, so only `toolKind` (or the name) says
 * it ran one (SV-03).
 */
function classifyToolAction(tool: CodingSessionTranscriptToolItem): ToolAction {
  const kind = tool.toolKind?.trim().toLowerCase() ?? null;
  if (kind === "execute") return "command";
  if (kind === "edit" || kind === "move" || kind === "delete") return "edit";
  if (kind === "search") return "code-search";
  if (kind === "read") return "read";
  const renderClass = baseRenderClass(tool);
  if (renderClass === "shell") return "command";
  if (renderClass === "file-edit") return "edit";
  if (renderClass === "file-read" || renderClass === "image") return "read";
  // The title stands in only when the provider sent no tool name. A name
  // that is present but server-qualified is someone else's tool, and its
  // title ("Read file") must not turn it into a local read.
  const name = tool.toolName?.trim()
    ? toolNameToken(tool.toolName)
    : toolNameToken(tool.title);
  return (name === null ? undefined : TOOL_NAME_ACTIONS.get(name)) ?? "other";
}

/** Provider tool names that say what the call did, as T3 reads them. */
const TOOL_NAME_ACTIONS: ReadonlyMap<string, ToolAction> = new Map([
  ["bash", "command"],
  ["shell", "command"],
  ["terminal", "command"],
  ["edit", "edit"],
  ["multiedit", "edit"],
  ["write", "edit"],
  ["notebookedit", "edit"],
  ["find", "code-search"],
  ["grep", "code-search"],
  ["glob", "code-search"],
  ["rg", "code-search"],
  ["ls", "code-search"],
  ["websearch", "web-search"],
  ["read", "read"],
  ["readfile", "read"],
]);

/**
 * A bare provider tool name, folded: `Read`, `read_file` and `read-file` all
 * read `readfile`. A server-qualified name (`github.read_file`,
 * `mcp__db__find`) is someone else's tool, never a local read or search.
 */
function toolNameToken(value: string | null | undefined): string | null {
  const trimmed = value?.trim();
  if (!trimmed || /__|[./`]/u.test(trimmed)) return null;
  return trimmed.replace(/[_\s-]/gu, "").toLowerCase();
}

/**
 * What the call was, regardless of how it ended. A failed call's descriptor
 * says only `error`; the fold counts it as the command (or read, or edit) it
 * attempted and names the failure separately.
 */
function baseRenderClass(tool: CodingSessionTranscriptToolItem) {
  const renderClass = tool.descriptor?.renderClass ?? tool.renderClass;
  if (renderClass !== "error") return renderClass;
  return classifyTool({
    title: tool.title,
    toolName: tool.toolName,
    buzzToolName: tool.buzzToolName,
    args: tool.args,
    result: tool.result,
    isError: false,
  }).renderClass;
}

function toolFileKey(tool: CodingSessionTranscriptToolItem): string | null {
  const path =
    getToolString(tool.args, [
      "path",
      "file",
      "file_path",
      "filePath",
      "target_file",
    ]) ??
    tool.descriptor?.object ??
    tool.descriptor?.action?.object ??
    null;
  const trimmed = path?.trim();
  return trimmed ? `path:${trimmed}` : null;
}
