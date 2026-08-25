import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";

export const MAX_CODING_SESSION_TASKS = 100;
export const MAX_CODING_SESSION_TASK_TEXT_LENGTH = 500;
export const MAX_CODING_SESSION_PLAN_EXPLANATION_LENGTH = 2_000;
export const MAX_CODING_SESSION_PLAN_COPY_LENGTH = 50_000;
const MAX_TRANSCRIPT_ITEMS_TO_SCAN = 2_000;

export type CodingSessionTaskStatus =
  | "pending"
  | "in_progress"
  | "completed"
  | "blocked"
  | "failed"
  | "cancelled"
  | "unknown";

export type CodingSessionTask = {
  id: string;
  text: string;
  status: CodingSessionTaskStatus;
  /** Signed elapsed time from this turn's first plan to first completion. */
  elapsedMs?: number;
};

export type CodingSessionTaskModel = {
  sourceItemId: string;
  turnId: string | null;
  timestamp: string;
  tasks: CodingSessionTask[];
  completedCount: number;
  explanation: string | null;
  copyText: string | null;
  state: "active" | "complete" | "empty";
};

type TaskInput = {
  text: string;
  status: CodingSessionTaskStatus;
};

type TaskSnapshot = {
  sourceItemId: string;
  turnId: string | null;
  timestamp: string;
  tasks: TaskInput[];
  explanation: string | null;
  copyText: string | null;
};

/**
 * Derive the latest task snapshot exclusively from the signed transcript.
 *
 * Plans are replacement snapshots, not an append-only task log. A recognized
 * empty array therefore intentionally clears an older plan. Malformed or
 * unrelated events are ignored instead of erasing the last valid snapshot.
 */
export function deriveCodingSessionTaskModel(
  transcript: TranscriptItem[],
): CodingSessionTaskModel | null {
  const firstIndex = Math.max(
    0,
    transcript.length - MAX_TRANSCRIPT_ITEMS_TO_SCAN,
  );

  for (let index = transcript.length - 1; index >= firstIndex; index -= 1) {
    const snapshot = extractTaskSnapshot(transcript[index]);
    if (!snapshot) continue;

    const tasks = attachSignedTaskElapsed(
      buildStableTasks(snapshot.tasks),
      transcript,
      snapshot,
    );
    const completedCount = tasks.filter(
      (task) => task.status === "completed",
    ).length;
    return {
      sourceItemId: snapshot.sourceItemId,
      turnId: snapshot.turnId,
      timestamp: snapshot.timestamp,
      tasks,
      completedCount,
      explanation: snapshot.explanation,
      copyText: snapshot.copyText,
      state:
        tasks.length === 0
          ? "empty"
          : completedCount === tasks.length
            ? "complete"
            : "active",
    };
  }

  return null;
}

function extractTaskSnapshot(item: TranscriptItem): TaskSnapshot | null {
  if (item.type === "plan") {
    const tasks = parseMarkdownChecklist(item.text);
    return tasks
      ? {
          sourceItemId: item.id,
          turnId: item.turnId ?? null,
          timestamp: item.timestamp,
          tasks,
          explanation: extractMarkdownExplanation(item.text),
          copyText: normalizeCopyText(item.text),
        }
      : null;
  }

  if (item.type !== "tool") return null;

  const toolKind = classifyTaskTool(item);
  if (!toolKind) return null;

  if (toolKind === "update_plan") {
    if (!Array.isArray(item.args.plan)) return null;
    const tasks = parseObjectTasks(item.args.plan, ["step", "content", "text"]);
    if (item.args.plan.length > 0 && tasks.length === 0) return null;
    const explanation = readExplanation(item.args);
    return {
      sourceItemId: item.id,
      turnId: item.turnId ?? null,
      timestamp: item.timestamp,
      tasks,
      explanation,
      copyText: buildTaskCopyText(explanation, tasks),
    };
  }

  if (Array.isArray(item.args.todos)) {
    const tasks = parseObjectTasks(item.args.todos, [
      "content",
      "text",
      "title",
      "label",
    ]);
    if (item.args.todos.length > 0 && tasks.length === 0) return null;
    const explanation = readExplanation(item.args);
    return {
      sourceItemId: item.id,
      turnId: item.turnId ?? null,
      timestamp: item.timestamp,
      tasks,
      explanation,
      copyText: buildTaskCopyText(explanation, tasks),
    };
  }

  const resultTasks = parseMarkdownChecklist(readToolResultText(item.result));
  return resultTasks
    ? {
        sourceItemId: item.id,
        turnId: item.turnId ?? null,
        timestamp: item.timestamp,
        tasks: resultTasks,
        explanation: null,
        copyText: buildTaskCopyText(null, resultTasks),
      }
    : null;
}

function classifyTaskTool(
  item: Extract<TranscriptItem, { type: "tool" }>,
): "update_plan" | "todo" | null {
  if (
    item.descriptor.groupKey === "plan:todo" ||
    item.descriptor.operation === "todo"
  ) {
    return "todo";
  }

  for (const name of [item.buzzToolName, item.toolName, item.title]) {
    const normalized = normalizeToolName(name);
    if (normalized === "update_plan" || normalized.endsWith("_update_plan")) {
      return "update_plan";
    }
    if (
      normalized === "todo" ||
      normalized === "todowrite" ||
      normalized.endsWith("_todo") ||
      normalized.endsWith("_todowrite")
    ) {
      return "todo";
    }
  }
  return null;
}

function normalizeToolName(value: string | null | undefined): string {
  return (value ?? "")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");
}

function parseObjectTasks(values: unknown[], textKeys: string[]): TaskInput[] {
  const tasks: TaskInput[] = [];
  for (const value of values) {
    if (tasks.length >= MAX_CODING_SESSION_TASKS) break;

    if (typeof value === "string") {
      const text = normalizeTaskText(value);
      if (text) tasks.push({ text, status: "unknown" });
      continue;
    }
    if (!value || typeof value !== "object" || Array.isArray(value)) continue;

    const record = value as Record<string, unknown>;
    const text = firstString(record, textKeys);
    if (!text) continue;
    tasks.push({
      text,
      status: readTaskStatus(record),
    });
  }
  return tasks;
}

function parseMarkdownChecklist(value: string): TaskInput[] | null {
  const tasks: TaskInput[] = [];
  let sawChecklist = false;

  for (const line of value.split(/\r?\n/)) {
    const match = line.match(/^\s*(?:[-*+]|\d+[.)])\s+\[([ xX])]\s+(.+?)\s*$/);
    if (!match) continue;
    sawChecklist = true;
    if (tasks.length >= MAX_CODING_SESSION_TASKS) continue;

    const inProgress = /\s+\(in progress\)\s*$/i.test(match[2]);
    const rawText = inProgress
      ? match[2].replace(/\s+\(in progress\)\s*$/i, "")
      : match[2];
    const text = normalizeTaskText(rawText);
    if (!text) continue;
    tasks.push({
      text,
      status:
        match[1].toLowerCase() === "x"
          ? "completed"
          : inProgress
            ? "in_progress"
            : "pending",
    });
  }

  return sawChecklist ? tasks : null;
}

function readTaskStatus(
  record: Record<string, unknown>,
): CodingSessionTaskStatus {
  if (typeof record.done === "boolean") {
    return record.done ? "completed" : "pending";
  }
  if (typeof record.checked === "boolean") {
    return record.checked ? "completed" : "pending";
  }

  const status = firstString(record, ["status"])?.toLowerCase();
  if (!status) return "unknown";
  const normalized = status.replace(/[\s-]+/g, "_");
  if (["completed", "done", "checked", "succeeded"].includes(normalized)) {
    return "completed";
  }
  if (["in_progress", "active", "current", "working"].includes(normalized)) {
    return "in_progress";
  }
  if (
    ["pending", "todo", "not_started", "open", "queued"].includes(normalized)
  ) {
    return "pending";
  }
  if (
    ["blocked", "stuck", "waiting", "waiting_for_input"].includes(normalized)
  ) {
    return "blocked";
  }
  if (["failed", "error", "errored"].includes(normalized)) {
    return "failed";
  }
  if (["cancelled", "canceled", "skipped"].includes(normalized)) {
    return "cancelled";
  }
  return "unknown";
}

function firstString(
  record: Record<string, unknown>,
  keys: string[],
): string | null {
  for (const key of keys) {
    if (typeof record[key] !== "string") continue;
    const text = normalizeTaskText(record[key]);
    if (text) return text;
  }
  return null;
}

function normalizeTaskText(value: string): string {
  return value
    .trim()
    .replace(/\s+/g, " ")
    .slice(0, MAX_CODING_SESSION_TASK_TEXT_LENGTH);
}

function readExplanation(record: Record<string, unknown>): string | null {
  if (typeof record.explanation !== "string") return null;
  return normalizeExplanation(record.explanation);
}

function extractMarkdownExplanation(value: string): string | null {
  const prose = value
    .split(/\r?\n/)
    .filter(
      (line) =>
        line.trim().length > 0 && !/^\s*(?:[-*+]|\d+[.)])\s+/.test(line),
    )
    .map((line) => line.replace(/^\s{0,3}#{1,6}\s+/, "").trim())
    .filter((line) => line.length > 0)
    .join(" ");
  return normalizeExplanation(prose);
}

function normalizeExplanation(value: string): string | null {
  const normalized = value.trim().replace(/\s+/g, " ");
  return normalized
    ? normalized.slice(0, MAX_CODING_SESSION_PLAN_EXPLANATION_LENGTH)
    : null;
}

function normalizeCopyText(value: string): string | null {
  const normalized = value.trim();
  return normalized
    ? normalized.slice(0, MAX_CODING_SESSION_PLAN_COPY_LENGTH)
    : null;
}

function buildTaskCopyText(
  explanation: string | null,
  tasks: TaskInput[],
): string | null {
  const lines = tasks.map((task) => {
    const marker = task.status === "completed" ? "x" : " ";
    const suffix =
      task.status === "in_progress"
        ? " (in progress)"
        : task.status === "pending" || task.status === "completed"
          ? ""
          : ` (${task.status})`;
    return `- [${marker}] ${task.text}${suffix}`;
  });
  return normalizeCopyText(
    [explanation, lines.length > 0 ? lines.join("\n") : null]
      .filter((value): value is string => value !== null)
      .join("\n\n"),
  );
}

function readToolResultText(result: string): string {
  try {
    const parsed: unknown = JSON.parse(result);
    if (typeof parsed === "string") return parsed;
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      const record = parsed as Record<string, unknown>;
      for (const key of ["stdout", "result", "text"]) {
        if (typeof record[key] === "string" && record[key].trim()) {
          return record[key];
        }
      }
    }
  } catch {
    // Plain-text tool results are expected.
  }
  return result;
}

function buildStableTasks(inputs: TaskInput[]): CodingSessionTask[] {
  const occurrences = new Map<string, number>();
  return inputs.map((input) => {
    const identity = input.text.toLowerCase();
    const occurrence = occurrences.get(identity) ?? 0;
    occurrences.set(identity, occurrence + 1);
    return {
      id: `task-${stableHash(identity)}-${occurrence}`,
      text: input.text,
      status: input.status,
    };
  });
}

function attachSignedTaskElapsed(
  tasks: CodingSessionTask[],
  transcript: TranscriptItem[],
  latest: TaskSnapshot,
): CodingSessionTask[] {
  if (!latest.turnId || !tasks.some((task) => task.status === "completed")) {
    return tasks;
  }

  const snapshots: TaskSnapshot[] = [];
  for (const item of transcript) {
    const snapshot = extractTaskSnapshot(item);
    if (snapshot?.turnId === latest.turnId) snapshots.push(snapshot);
    if (snapshot?.sourceItemId === latest.sourceItemId) break;
  }
  const startedAt = Date.parse(snapshots[0]?.timestamp ?? "");
  if (!Number.isFinite(startedAt)) return tasks;

  const completedAt = new Map<string, number>();
  for (const snapshot of snapshots) {
    const observedAt = Date.parse(snapshot.timestamp);
    if (!Number.isFinite(observedAt)) continue;
    for (const task of buildStableTasks(snapshot.tasks)) {
      if (task.status === "completed" && !completedAt.has(task.id)) {
        completedAt.set(task.id, observedAt);
      }
    }
  }

  return tasks.map((task) => {
    if (task.status !== "completed") return task;
    const finishedAt = completedAt.get(task.id);
    if (finishedAt === undefined || finishedAt < startedAt) return task;
    return { ...task, elapsedMs: finishedAt - startedAt };
  });
}

function stableHash(value: string): string {
  let hash = 0x811c9dc5;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0).toString(36);
}
