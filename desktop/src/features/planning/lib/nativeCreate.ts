/**
 * Creating a native issue: a list if the project has none, one `item.add`,
 * then one op per further field. Separate signed events, so no all-or-nothing
 * claim is made: once the `item.add` is accepted the issue exists, every
 * later field is attempted and a refused one is returned in `failed` — never
 * thrown, because a caller that saw a rejection would keep the draft and a
 * retry would mint a second issue.
 */
import type { TodoMutations } from "../../project-todos/lib/todoMutations.ts";
import type {
  TodoPriority,
  TodoStatus,
} from "../../project-todos/lib/todoOp.ts";

import { DEFAULT_NATIVE_CONTAINER_TITLE } from "./nativePlanning.ts";
import type {
  PlanningCreateInput,
  PlanningCreateResult,
} from "./planningTypes.ts";

export type NativeCreateWrites = Pick<
  TodoMutations,
  | "createList"
  | "addItem"
  | "setAssignee"
  | "setStatus"
  | "setPriority"
  | "setDescription"
>;

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Create `input` as a native issue. Rejects only while nothing exists yet
 * (the list create or the `item.add` refused); after that it resolves with
 * the issue's key and the fields that did not save.
 */
export async function createNativeIssue(
  writes: NativeCreateWrites,
  input: PlanningCreateInput & {
    statusId: TodoStatus;
    priorityId: TodoPriority;
  },
): Promise<{
  listId: string;
  itemId: string;
  failed: PlanningCreateResult["failed"];
}> {
  const listId =
    input.containerId ??
    (await writes.createList(DEFAULT_NATIVE_CONTAINER_TITLE, "project"));
  const itemId = await writes.addItem(listId, input.title);
  const failed: PlanningCreateResult["failed"] = [];
  const attempt = async (field: string, work: () => Promise<void>) => {
    try {
      await work();
    } catch (error) {
      failed.push({ field, message: message(error) });
    }
  };
  const { assignee, statusId, priorityId, description } = input;
  if (assignee !== null) {
    await attempt("assignee", () =>
      writes.setAssignee(listId, itemId, assignee),
    );
  }
  if (statusId !== "todo") {
    await attempt("status", () => writes.setStatus(listId, itemId, statusId));
  }
  if (priorityId !== "none") {
    await attempt("priority", () =>
      writes.setPriority(listId, itemId, priorityId),
    );
  }
  if (description !== null) {
    await attempt("description", () =>
      writes.setDescription(listId, itemId, description),
    );
  }
  return { listId, itemId, failed };
}
