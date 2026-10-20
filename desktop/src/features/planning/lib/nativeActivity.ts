/**
 * An issue's history, read straight from the ops that wrote it. Each entry
 * is one signed kind 44248 event on the relay — who did what, and when — not
 * a summary the client composed. An op that lost a same-field race still
 * happened, so it is listed; the fields above the history show who won.
 */
import type { TodoFoldEvent } from "../../project-todos/lib/todoFold.ts";
import {
  decodeTodoOp,
  isTodoVisibility,
  type TodoOp,
} from "../../project-todos/lib/todoOp.ts";
import { nativePriority, nativeStatus } from "./nativePlanning.ts";

export type ActivityEntry = {
  id: string;
  actor: string;
  createdAt: number;
  /** What changed, without the actor: "set status to In Progress". */
  summary: string;
  /** The person an assignment names, so the UI can render their name. */
  subject?: string | null;
};

function describe(
  op: TodoOp,
): Omit<ActivityEntry, "id" | "actor" | "createdAt"> | null {
  switch (op.op) {
    case "item.add":
      return { summary: "created the issue" };
    case "item.text":
      return { summary: "renamed the issue" };
    case "item.description":
      return {
        summary:
          op.description === null
            ? "cleared the description"
            : "edited the description",
      };
    case "item.status":
      return { summary: `set status to ${nativeStatus(op.status).label}` };
    case "item.done":
      return { summary: op.done ? "marked it done" : "marked it not done" };
    case "item.priority":
      return {
        summary:
          op.priority === "none"
            ? "removed the priority"
            : `set priority to ${nativePriority(op.priority).label}`,
      };
    case "item.assignee":
      return op.assignee === null
        ? { summary: "unassigned it" }
        : { summary: "assigned", subject: op.assignee };
    case "item.due":
      return {
        summary: op.due === null ? "cleared the due date" : `set due ${op.due}`,
      };
    case "item.rank":
      return { summary: "moved it" };
    default:
      return null;
  }
}

/** Every decodable op on `(listId, itemId)`, oldest first. */
export function nativeIssueActivity(
  events: readonly TodoFoldEvent[],
  listId: string,
  itemId: string,
): ActivityEntry[] {
  const entries: { key: [number, string]; entry: ActivityEntry }[] = [];
  const seen = new Set<string>();
  for (const event of events) {
    if (seen.has(event.id)) continue;
    seen.add(event.id);
    const tag = (name: string) => event.tags.find((t) => t[0] === name)?.[1];
    if (tag("td-list") !== listId || tag("td-item") !== itemId) continue;
    // Planning shows project lists only; an op tagged otherwise is one the
    // fold ignores, so the history leaves it out too.
    const visibility = tag("td-vis");
    if (!isTodoVisibility(visibility) || visibility !== "project") continue;
    const op = decodeTodoOp(event.content, visibility);
    if ("error" in op) continue;
    const described = describe(op);
    if (!described) continue;
    entries.push({
      key: [event.created_at, event.id],
      entry: {
        id: event.id,
        actor: event.pubkey,
        createdAt: event.created_at,
        ...described,
      },
    });
  }
  entries.sort((a, b) =>
    a.key[0] !== b.key[0]
      ? a.key[0] - b.key[0]
      : a.key[1] < b.key[1]
        ? -1
        : a.key[1] > b.key[1]
          ? 1
          : 0,
  );
  return entries.map((e) => e.entry);
}
