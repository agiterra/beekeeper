import * as React from "react";

import type { ProjectContainer } from "@/features/projects-container/lib/projectContainerModel";

import type { TodoWriteAccess } from "../lib/todoAccess";
import type { TodoItem, TodoList } from "../lib/todoFold";
import type { TodoMutations } from "../lib/todoMutations";
import type { TodoPerson } from "../lib/todoPeople";
import type { ProjectTodosState } from "../lib/todoQueries";
import type { TodoRowActions } from "./TodoItemRow";
import { TodoListPanel } from "./TodoListPanel";
import { TodoListPicker } from "./TodoListPicker";

export type ProjectTodosViewProps = {
  project: ProjectContainer;
  state: ProjectTodosState;
  access: TodoWriteAccess;
  mutations: TodoMutations;
  personFor: (pubkey: string) => TodoPerson;
  /** Surface a write failure; the view never swallows one. */
  onWriteError: (message: string) => void;
};

/**
 * The To-Do tab's body: notices (read-only, truncated, ignored, errors), the
 * list rail, and the selected list. Presentational apart from the selected
 * list and the archived toggle, which are view state.
 */
export function ProjectTodosView({
  project,
  state,
  access,
  mutations,
  personFor,
  onWriteError,
}: ProjectTodosViewProps) {
  const read = state.read;
  const lists: readonly TodoList[] = read?.digest.lists ?? [];
  const [selectedId, setSelectedId] = React.useState<string | null>(null);
  const [showArchived, setShowArchived] = React.useState(false);

  // Default to the first unarchived list, and follow a list the viewer
  // created; never point at a list that no longer exists.
  const effectiveSelectedId = React.useMemo(() => {
    if (selectedId && lists.some((list) => list.id === selectedId))
      return selectedId;
    return lists.find((list) => !list.archived)?.id ?? lists[0]?.id ?? null;
  }, [lists, selectedId]);
  const selected =
    lists.find((list) => list.id === effectiveSelectedId) ?? null;
  const canEdit = access.kind === "writable";

  const run = React.useCallback(
    (work: Promise<unknown>) => {
      work.catch((error: unknown) => {
        onWriteError(error instanceof Error ? error.message : String(error));
      });
    },
    [onWriteError],
  );

  const actions = React.useMemo<TodoRowActions>(
    () => ({
      setDone: (item: TodoItem, done: boolean) =>
        run(mutations.setDone(item.listId, item.id, done)),
      setText: (item, text) =>
        run(mutations.setText(item.listId, item.id, text)),
      setAssignee: (item, assignee) =>
        run(mutations.setAssignee(item.listId, item.id, assignee)),
      setDue: (item, due) => run(mutations.setDue(item.listId, item.id, due)),
      remove: (item) => run(mutations.removeItem(item.listId, item.id)),
    }),
    [mutations, run],
  );

  const notices: { key: string; text: string; tone: "info" | "warn" }[] = [];
  if (access.kind === "read-only") {
    notices.push({
      key: "read-only",
      text: `Read-only: ${access.reason}`,
      tone: "info",
    });
  }
  if (access.kind === "no-coordinate") {
    notices.push({
      key: "no-coordinate",
      text: "This project has no published head yet, so it cannot hold to-do lists. Publish it first.",
      tone: "info",
    });
  }
  if (read?.truncated) {
    notices.push({
      key: "truncated",
      text: "This read stopped early: the relay returned more history than was fetched, so older items may be missing.",
      tone: "warn",
    });
  }
  if (read && read.digest.ignored > 0) {
    notices.push({
      key: "ignored",
      text: `${read.digest.ignored} change${read.digest.ignored === 1 ? "" : "s"} could not be applied (malformed, or naming a list or item that does not exist).`,
      tone: "warn",
    });
  }
  if (state.kind === "error") {
    notices.push({
      key: "error",
      text: `Could not read the lists: ${state.message}`,
      tone: "warn",
    });
  }

  return (
    <div
      className="flex min-h-0 flex-1 flex-col gap-3"
      data-testid="project-todos-view"
    >
      {notices.length > 0 ? (
        <ul className="flex flex-col gap-1" data-testid="todo-notices">
          {notices.map((notice) => (
            <li
              className={
                notice.tone === "warn"
                  ? "rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs text-foreground"
                  : "rounded-md border border-border bg-muted/40 px-3 py-2 text-xs text-muted-foreground"
              }
              data-testid={`todo-notice-${notice.key}`}
              key={notice.key}
            >
              {notice.text}
            </li>
          ))}
        </ul>
      ) : null}
      {state.kind === "loading" && !read ? (
        <p className="text-sm text-muted-foreground" data-testid="todo-loading">
          Reading lists…
        </p>
      ) : (
        <div className="flex min-h-0 flex-1 gap-4">
          <TodoListPicker
            canEdit={canEdit}
            lists={lists}
            onCreate={(title) =>
              run(
                mutations.createList(title).then((listId) => {
                  setSelectedId(listId);
                }),
              )
            }
            onRename={(listId, title) =>
              run(mutations.renameList(listId, title))
            }
            onSelect={setSelectedId}
            onSetArchived={(listId, archived) =>
              run(mutations.setListArchived(listId, archived))
            }
            onToggleArchived={() => setShowArchived((value) => !value)}
            selectedId={effectiveSelectedId}
            showArchived={showArchived}
          />
          {selected ? (
            <TodoListPanel
              actions={actions}
              canEdit={canEdit && !selected.archived}
              list={selected}
              onAdd={(text) => run(mutations.addItem(selected.id, text))}
              onMove={(item, index) =>
                run(mutations.moveItem(item.listId, item.id, index))
              }
              personFor={personFor}
              project={project}
            />
          ) : (
            <p
              className="text-sm text-muted-foreground"
              data-testid="todo-no-list"
            >
              {canEdit
                ? "Create a list to get started."
                : "This project has no to-do lists yet."}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
