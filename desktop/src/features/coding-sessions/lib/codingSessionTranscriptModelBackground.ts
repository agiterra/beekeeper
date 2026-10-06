import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import { isCodingSessionContinuityItem } from "@/features/coding-sessions/lib/codingSessionTranscriptModelPredicates";
import type {
  CodingSessionTurnAutonomousWake,
  CodingSessionTurnBackgroundTask,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModelTypes";

/**
 * Background tasks a turn left behind, and the notifications that report
 * them (SV-78, ledger 336).
 *
 * Read from the transcript's own words only. Claude Code's Bash tool answers
 * a backgrounded command with "Command running in background with ID: <id>.
 * … You will be notified when it completes." and later wakes the agent with
 * a `<task-notification>` block naming that id and its `<status>`. Nothing
 * else counts: a turn that ended with neither phrase started no background
 * task as far as anyone can show, and one whose notification never arrived
 * is still running — or, once a later execution began, was never reported
 * finished. Never "finished": the transcript cannot say so.
 *
 * claude-agent-acp does not forward the notification text to a client that
 * is not JetBrains AIR (verified live against 0.84.0, ledger 336): the agent
 * wakes, answers, and the transcript holds only the provider's
 * `autonomous_turn…` status rows (SV-77). A task followed by such a wake is
 * therefore `woke`: something woke the agent, the transcript cannot say
 * whether it was this task, and "running" would be a claim nobody can back.
 */

/** Claude Code 2.1.x's Bash tool result for a backgrounded command. */
const BACKGROUND_COMMAND_STARTED =
  /Command running in background with ID: ([A-Za-z0-9_-]+)/g;

const TASK_NOTIFICATION_BLOCK =
  /<task-notification>([\s\S]*?)<\/task-notification>/g;

/** One `<task-notification>` block's fields. Absent fields are `null`. */
export type CodingSessionTaskNotification = {
  taskId: string;
  status: string | null;
  summary: string | null;
};

/** The ids a backgrounded command's tool result announced, in order. */
export function parseCodingSessionBackgroundTaskStarts(text: string): string[] {
  const ids: string[] = [];
  if (!text.includes("Command running in background")) return ids;
  for (const match of text.matchAll(BACKGROUND_COMMAND_STARTED)) {
    const id = match[1];
    if (id && !ids.includes(id)) ids.push(id);
  }
  return ids;
}

/** The provider's status rows around a turn nobody prompted (SV-77). */
export function isCodingSessionAutonomousWakeRow(
  item: TranscriptItem,
): boolean {
  return (
    item.type === "lifecycle" &&
    item.title === "Status" &&
    item.text.startsWith("autonomous_turn")
  );
}

/**
 * Human wording for a provider status row's text in "Turn details" (SV-93),
 * matching the turn's "Woke on its own · background task" marker. Only the
 * codes this app knows are reworded; anything else stays verbatim, since a
 * guessed gloss on an unknown code would be a claim we cannot back.
 */
export function describeCodingSessionStatusText(text: string): string {
  const code = text.split(":", 1)[0]?.trim() ?? "";
  if (code === "autonomous_turn_started") {
    return "Woke on its own · began a turn nobody prompted";
  }
  if (code === "autonomous_turn") {
    return text.includes("task-notification")
      ? "Woke on its own · background task"
      : "Woke on its own";
  }
  return text;
}

/** Every complete `<task-notification>` block in `text` that names a task. */
export function parseCodingSessionTaskNotifications(
  text: string,
): CodingSessionTaskNotification[] {
  if (!text.includes("<task-notification>")) return [];
  const notifications: CodingSessionTaskNotification[] = [];
  for (const block of text.matchAll(TASK_NOTIFICATION_BLOCK)) {
    const body = block[1] ?? "";
    const taskId = readTag(body, "task-id");
    if (!taskId) continue;
    notifications.push({
      taskId,
      status: readTag(body, "status"),
      summary: readTag(body, "summary"),
    });
  }
  return notifications;
}

/**
 * A prompt-role message that is wholly a task notification: the agent's
 * runtime woke it, nobody typed it. The turn renders it as a wake row, never
 * as a person's bubble.
 */
export function isCodingSessionTaskNotificationItem(
  item: TranscriptItem,
): boolean {
  if (item.type !== "message" || item.role !== "user") return false;
  const text = item.text.trim();
  return (
    text.startsWith("<task-notification>") &&
    text.endsWith("</task-notification>") &&
    parseCodingSessionTaskNotifications(text).length > 0
  );
}

/**
 * The background tasks each item started, resolved against everything after
 * it in `transcript`. Keyed by the starting tool item; a turn reads its own.
 *
 * - `reported`: a later notification names the id (its status is kept).
 * - `woke`: no notification, but the agent later started a turn nobody
 *   prompted — possibly on this task's notification, which the adapter does
 *   not show us.
 * - `unreported`: no notification, but a later session-continuity row says a
 *   new execution began, and the process that would have reported it is gone.
 * - `running`: neither — nothing on screen says it ended.
 *
 * Subagent items are skipped: a subagent's background task reports to the
 * subagent, not to the turn.
 */
export function deriveCodingSessionBackgroundTasks(
  transcript: readonly TranscriptItem[],
): ReadonlyMap<TranscriptItem, readonly CodingSessionTurnBackgroundTask[]> {
  const starts: Array<{ item: TranscriptItem; index: number; ids: string[] }> =
    [];
  const reportedAt = new Map<
    string,
    { index: number; status: string | null }
  >();
  let lastContinuity = -1;
  let lastWake = -1;

  transcript.forEach((item, index) => {
    if (isCodingSessionContinuityItem(item)) lastContinuity = index;
    if (isCodingSessionAutonomousWakeRow(item)) lastWake = index;
    const text = searchableText(item);
    if (!text) return;
    if (item.type === "tool" && !item.parentToolId) {
      const ids = parseCodingSessionBackgroundTaskStarts(text);
      if (ids.length > 0) starts.push({ item, index, ids });
    }
    for (const notification of parseCodingSessionTaskNotifications(text)) {
      // The latest report wins: a task resumed and settled again reads as
      // its last word.
      reportedAt.set(notification.taskId, {
        index,
        status: notification.status,
      });
    }
  });

  const byItem = new Map<TranscriptItem, CodingSessionTurnBackgroundTask[]>();
  for (const start of starts) {
    byItem.set(
      start.item,
      start.ids.map((id) => {
        const report = reportedAt.get(id);
        if (report && report.index > start.index) {
          return { id, state: "reported", status: report.status };
        }
        if (lastWake > start.index) {
          return { id, state: "woke", status: null };
        }
        if (lastContinuity > start.index) {
          return { id, state: "unreported", status: null };
        }
        return { id, state: "running", status: null };
      }),
    );
  }
  return byItem;
}

const NO_TASKS: readonly CodingSessionTurnBackgroundTask[] = [];
const EMPTY_BY_TURN: ReadonlyMap<
  string,
  readonly CodingSessionTurnBackgroundTask[]
> = new Map();

/**
 * One derivation per transcript array, however many blocks read it — keyed by
 * item id, not identity, so a block whose items were rebuilt from the same
 * events still finds its tasks.
 */
const derivedByTranscript = new WeakMap<
  readonly TranscriptItem[],
  ReadonlyMap<string, readonly CodingSessionTurnBackgroundTask[]>
>();

function deriveCached(
  transcript: readonly TranscriptItem[],
): ReadonlyMap<string, readonly CodingSessionTurnBackgroundTask[]> {
  let derived = derivedByTranscript.get(transcript);
  if (!derived) {
    const byId = new Map<string, readonly CodingSessionTurnBackgroundTask[]>();
    for (const [item, tasks] of deriveCodingSessionBackgroundTasks(
      transcript,
    )) {
      byId.set(item.id, tasks);
    }
    derived = byId;
    derivedByTranscript.set(transcript, derived);
  }
  return derived;
}

/**
 * The background tasks one umbrella turn block started, keyed by turn id, and
 * resolved against its **whole** generation transcript (SV-91).
 *
 * A block is a window into one execution's stream. Derived over the block
 * alone, its task can never see the next block's `autonomous_turn…` row, so
 * it would read "running" for ever; and Mission Live moves the block's tool
 * items into the execution bundle, so the narrative's own model never sees
 * the Bash result that announced the task at all. The caller hands the result
 * to the narrative's model, which then says it whatever items it holds.
 *
 * Only the tasks whose starting item is in `blockItems` count, so a turn split
 * across two blocks says it once, on the block that started the work.
 *
 * `generationSuperseded`: a later generation of this execution exists. The
 * process that would have reported a task this one left running is gone, so
 * `running` reads `unreported` — exactly what a continuity row says inside
 * one transcript, which is never in this generation's own.
 *
 * Returns one shared empty map when the block started nothing.
 */
export function deriveCodingSessionBlockBackgroundTasks(input: {
  transcript: readonly TranscriptItem[];
  blockItems: readonly TranscriptItem[];
  generationSuperseded: boolean;
}): ReadonlyMap<string, readonly CodingSessionTurnBackgroundTask[]> {
  const derived = deriveCached(input.transcript);
  if (derived.size === 0) return EMPTY_BY_TURN;
  const byTurn = new Map<string, CodingSessionTurnBackgroundTask[]>();
  for (const item of input.blockItems) {
    const tasks = derived.get(item.id);
    if (!tasks || !item.turnId) continue;
    const list = byTurn.get(item.turnId) ?? [];
    for (const task of tasks) {
      list.push(
        input.generationSuperseded && task.state === "running"
          ? { ...task, state: "unreported" }
          : task,
      );
    }
    byTurn.set(item.turnId, list);
  }
  return byTurn.size === 0 ? EMPTY_BY_TURN : byTurn;
}

/** Same turns, same tasks, same states — so a caller can keep its old map. */
export function codingSessionBackgroundTasksByTurnEqual(
  left: ReadonlyMap<string, readonly CodingSessionTurnBackgroundTask[]>,
  right: ReadonlyMap<string, readonly CodingSessionTurnBackgroundTask[]>,
): boolean {
  if (left === right) return true;
  if (left.size !== right.size) return false;
  for (const [turnId, tasks] of left) {
    const other = right.get(turnId);
    if (!other || !codingSessionBackgroundTasksEqual(tasks, other)) {
      return false;
    }
  }
  return true;
}

export function codingSessionBackgroundTasksEqual(
  left: readonly CodingSessionTurnBackgroundTask[],
  right: readonly CodingSessionTurnBackgroundTask[],
): boolean {
  return (
    left.length === right.length &&
    left.every((task, index) => {
      const other = right[index];
      return (
        other !== undefined &&
        task.id === other.id &&
        task.state === other.state &&
        task.status === other.status
      );
    })
  );
}

/** Does any turn in `byTurn` hold a task nothing says has ended? */
export function hasRunningCodingSessionBackgroundTask(
  byTurn: ReadonlyMap<string, readonly CodingSessionTurnBackgroundTask[]>,
): boolean {
  for (const tasks of byTurn.values()) {
    if (tasks.some((task) => task.state === "running")) return true;
  }
  return false;
}

/** A turn's tasks from a caller-derived map; the shared empty list if none. */
export function codingSessionTurnBackgroundTasksFrom(
  byTurn: ReadonlyMap<string, readonly CodingSessionTurnBackgroundTask[]>,
  turnId: string,
): readonly CodingSessionTurnBackgroundTask[] {
  return byTurn.get(turnId) ?? NO_TASKS;
}

/**
 * What the provider's status rows say about how a turn began (SV-93), or
 * `null` when none says nobody prompted it.
 *
 * claude-agent-acp 0.84.0 never forwards the `<task-notification>` prompt to
 * us (ledger 336), so these rows are the only evidence on the wire: an
 * `autonomous_turn_started` row opens the turn, and a later `autonomous_turn:
 * the agent woke on task-notification` row names the cause — it can arrive
 * mid-answer, so the cause may fill in after the marker first shows. A turn
 * that carries a task-notification message is left alone: that message's
 * own row already says what woke it.
 */
export function deriveCodingSessionTurnAutonomousWake(
  items: readonly TranscriptItem[],
): CodingSessionTurnAutonomousWake | null {
  let timestamp: string | null = null;
  let cause: CodingSessionTurnAutonomousWake["cause"] = null;
  for (const item of items) {
    if (isCodingSessionTaskNotificationItem(item)) return null;
    if (!isCodingSessionAutonomousWakeRow(item) || item.type !== "lifecycle") {
      continue;
    }
    timestamp ??= item.timestamp;
    if (item.text.includes("task-notification")) cause = "background-task";
  }
  return timestamp === null ? null : { cause, timestamp };
}

/**
 * The turn-row clause for the tasks still outstanding, or `null` when none
 * is. `sessionEnded` turns "running" into "never reported finished": once the
 * session is over nothing will report them, and saying they finished would be
 * a guess.
 */
export function formatCodingSessionBackgroundTasks(
  tasks: readonly CodingSessionTurnBackgroundTask[],
  sessionEnded = false,
): string | null {
  let running = 0;
  let unreported = 0;
  let woke = 0;
  for (const task of tasks) {
    if (task.state === "running" && !sessionEnded) running += 1;
    else if (task.state === "woke") woke += 1;
    else if (task.state !== "reported") unreported += 1;
  }
  const noun = (count: number) =>
    `${count} background ${count === 1 ? "task" : "tasks"}`;
  const clauses: string[] = [];
  if (running > 0) clauses.push(`${noun(running)} running`);
  if (unreported > 0) {
    clauses.push(`${noun(unreported)} never reported finished`);
  }
  if (woke > 0) {
    clauses.push(`${noun(woke)}, then the agent woke on its own`);
  }
  return clauses.length > 0 ? clauses.join(" · ") : null;
}

/** The ids behind {@link formatCodingSessionBackgroundTasks}, for a title. */
export function outstandingCodingSessionBackgroundTaskIds(
  tasks: readonly CodingSessionTurnBackgroundTask[],
): string[] {
  return tasks.filter((task) => task.state !== "reported").map((t) => t.id);
}

function searchableText(item: TranscriptItem): string {
  switch (item.type) {
    case "tool":
      return item.result;
    case "message":
    case "lifecycle":
      return item.text;
    default:
      return "";
  }
}

function readTag(body: string, tag: string): string | null {
  const open = `<${tag}>`;
  const start = body.indexOf(open);
  if (start < 0) return null;
  const end = body.indexOf(`</${tag}>`, start + open.length);
  if (end < 0) return null;
  const value = body.slice(start + open.length, end).trim();
  return value || null;
}
