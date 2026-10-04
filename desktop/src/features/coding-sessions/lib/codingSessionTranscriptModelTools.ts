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
import { isCompletedSuccessfulTool } from "@/features/coding-sessions/lib/codingSessionTranscriptModelPredicates";
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

export function groupAdjacentTools(
  items: TranscriptItem[],
  subagents: CodingSessionSubagentPartition,
): CodingSessionTranscriptEntry[] {
  const narrativeItems = coalescePlanSnapshots(items);
  const entries: CodingSessionTranscriptEntry[] = [];
  const isSpawn = (candidate: TranscriptItem | undefined) =>
    candidate !== undefined &&
    isCodingSessionSubagentCall(candidate, subagents);
  const isGroupable = (candidate: TranscriptItem | undefined) =>
    candidate !== undefined &&
    isCompletedSuccessfulTool(candidate) &&
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
    if (!isGroupable(item) || item.type !== "tool") {
      entries.push({ kind: "item", item });
      continue;
    }

    const tools: CodingSessionTranscriptToolItem[] = [item];
    let cursor = index + 1;
    while (cursor < narrativeItems.length) {
      const candidate = narrativeItems[cursor];
      if (!isGroupable(candidate) || candidate?.type !== "tool") break;
      tools.push(candidate);
      cursor += 1;
    }

    if (tools.length >= TOOL_GROUP_MIN) {
      entries.push({
        kind: "tool-group",
        // Keyed by the first call, so the row stays mounted (and keeps its
        // open state) while a live run grows behind it.
        id: `tools:${item.id}`,
        label: summarizeCodingSessionTools(tools),
        items: tools,
      });
    } else {
      entries.push({ kind: "item", item });
    }
    index = cursor - 1;
  }

  return entries;
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
  | "search"
  | "skill"
  | "image"
  | "other";

const TOOL_ACTION_NOUNS: Record<
  ToolAction,
  { verb: string; one: string; many: string }
> = {
  read: { verb: "Read", one: "file", many: "files" },
  edit: { verb: "Edited", one: "file", many: "files" },
  command: { verb: "Ran", one: "command", many: "commands" },
  search: { verb: "Ran", one: "search", many: "searches" },
  skill: { verb: "Read", one: "skill", many: "skills" },
  image: { verb: "Viewed", one: "image", many: "images" },
  other: { verb: "Ran", one: "tool call", many: "tool calls" },
};

/**
 * One sentence for a run of tool calls: "Read 3 files, ran 2 commands and
 * edited 1 file". Clauses follow the order each kind first appears.
 *
 * Files are counted once each — three reads of one file read one file — and
 * everything else is counted per call. A call that names no file counts as its
 * own, so the number never claims fewer files than the calls could have
 * touched. Mixed unknown tools say "tool calls" rather than guessing a verb.
 */
export function summarizeCodingSessionTools(
  tools: readonly CodingSessionTranscriptToolItem[],
): string {
  const buckets = new Map<ToolAction, Set<string>>();
  for (const tool of tools) {
    const action = classifyToolAction(tool);
    const key =
      action === "read" || action === "edit"
        ? (toolFileKey(tool) ?? `call:${tool.id}`)
        : `call:${tool.id}`;
    const bucket = buckets.get(action) ?? new Set<string>();
    bucket.add(key);
    buckets.set(action, bucket);
  }
  const clauses = [...buckets].map(([action, keys], index) => {
    const nouns = TOOL_ACTION_NOUNS[action];
    const verb = index === 0 ? nouns.verb : nouns.verb.toLowerCase();
    return `${verb} ${keys.size} ${keys.size === 1 ? nouns.one : nouns.many}`;
  });
  if (clauses.length < 2) return clauses[0] ?? "";
  return `${clauses.slice(0, -1).join(", ")} and ${clauses.at(-1)}`;
}

function classifyToolAction(tool: CodingSessionTranscriptToolItem): ToolAction {
  // ACP's own discriminant outranks any name rule: claude-agent-acp titles a
  // Bash call with its command, so only `toolKind` says it ran one (SV-03).
  if (tool.toolKind === "execute") return "command";
  if (tool.toolKind === "edit") return "edit";
  const renderClass = baseRenderClass(tool);
  if (renderClass === "file-edit") return "edit";
  if (renderClass === "file-read") return "read";
  if (renderClass === "shell") return "command";
  if (renderClass === "skill-read") return "skill";
  if (renderClass === "image") return "image";
  if (
    tool.toolKind === "search" ||
    tool.descriptor?.action?.verb === "Searched"
  ) {
    return "search";
  }
  if (tool.toolKind === "read") return "read";
  return "other";
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
