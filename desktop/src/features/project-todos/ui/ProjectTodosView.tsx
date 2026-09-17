import { Link } from "@tanstack/react-router";
import { Lock, Pin } from "lucide-react";
import * as React from "react";

import type { ProjectContainer } from "@/features/projects-container/lib/projectContainerModel";

import type { TodoWriteAccess } from "../lib/todoAccess";
import type { TodoItem, TodoList } from "../lib/todoFold";
import type { TodoMutations } from "../lib/todoMutations";
import type { TodoPerson } from "../lib/todoPeople";
import type { ProjectTodosState } from "../lib/todoQueries";
import { CreateTodoListDialog } from "./CreateTodoListDialog";
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
  /** The list the route names, or null; the view falls back sensibly. */
  selectedListId?: string | null;
  /** Called when the person picks a list, so the route can carry it. */
  onSelectList?: (listId: string | null) => void;
  /** Show only the selected list — no rail — the way a sidebar row opens it. */
  focused?: boolean;
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
  selectedListId = null,
  onSelectList,
  focused = false,
}: ProjectTodosViewProps) {
  const read = state.read;
  const lists: readonly TodoList[] = read?.digest.lists ?? [];
  const [localSelectedId, setLocalSelectedId] = React.useState<string | null>(
    null,
  );
  // The route wins when it names a list; otherwise the last local pick.
  const selectedId = selectedListId ?? localSelectedId;
  const setSelectedId = React.useCallback(
    (listId: string | null) => {
      setLocalSelectedId(listId);
      onSelectList?.(listId);
    },
    [onSelectList],
  );
  const [showArchived, setShowArchived] = React.useState(false);
  const [creating, setCreating] = React.useState(false);
  const [createPending, setCreatePending] = React.useState(false);

  // Default to the first unarchived list, and follow a list the viewer
  // created; never point at a list that no longer exists.
  const effectiveSelectedId = React.useMemo(() => {
    if (selectedId && lists.some((list) => list.id === selectedId))
      return selectedId;
    // A focused view names one list and never stands in another for it.
    if (focused) return null;
    return lists.find((list) => !list.archived)?.id ?? lists[0]?.id ?? null;
  }, [focused, lists, selectedId]);
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
  if (read && read.legacy > 0) {
    notices.push({
      key: "legacy",
      text: `${read.legacy} change${read.legacy === 1 ? "" : "s"} from an older build of this feature ${read.legacy === 1 ? "was" : "were"} skipped: ${read.legacy === 1 ? "it predates" : "they predate"} list visibility and cannot be read safely.`,
      tone: "info",
    });
  }
  const unexplained = read ? read.digest.ignored - read.legacy : 0;
  if (unexplained > 0) {
    notices.push({
      key: "ignored",
      text: `${unexplained} change${unexplained === 1 ? "" : "s"} could not be applied (malformed, or naming a list or item that does not exist).`,
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
      ) : focused ? (
        <div
          className="flex min-h-0 flex-1 flex-col gap-3"
          data-testid="todo-focused"
        >
          <div className="flex items-center gap-2">
            <h1 className="min-w-0 truncate text-xl font-semibold text-foreground">
              {selected ? selected.title : "List not found"}
            </h1>
            {selected?.visibility === "personal" ? (
              <Lock
                aria-label="Personal"
                className="h-3.5 w-3.5 shrink-0 text-muted-foreground"
              />
            ) : null}
            {selected?.pinned ? (
              <Pin
                aria-label="Pinned to the sidebar"
                className="h-3.5 w-3.5 shrink-0 text-muted-foreground"
              />
            ) : null}
            <span className="flex-1" />
            <Link
              className="text-xs text-muted-foreground hover:text-foreground hover:underline"
              data-testid="todo-focused-all-lists"
              params={{ projectId: project.id }}
              search={{}}
              to="/projects/$projectId/todos"
            >
              All lists
            </Link>
          </div>
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
              This list is not in the current read; it may have been removed or
              not be visible to you.
            </p>
          )}
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 gap-4">
          <TodoListPicker
            canEdit={canEdit}
            lists={lists}
            onCreate={() => setCreating(true)}
            onRename={(listId, title) =>
              run(mutations.renameList(listId, title))
            }
            onSelect={setSelectedId}
            onSetArchived={(listId, archived) =>
              run(mutations.setListArchived(listId, archived))
            }
            onSetPinned={(listId, pinned) =>
              run(mutations.setListPinned(listId, pinned))
            }
            onToggleArchived={() => setShowArchived((value) => !value)}
            selectedId={effectiveSelectedId}
            showArchived={showArchived}
          />
          <CreateTodoListDialog
            isCreating={createPending}
            onCreate={async (input) => {
              setCreatePending(true);
              try {
                const listId = await mutations.createList(
                  input.title,
                  input.visibility,
                  { pinned: input.pinned },
                );
                setSelectedId(listId);
              } finally {
                setCreatePending(false);
              }
            }}
            onOpenChange={setCreating}
            open={creating}
            projectName={project.name}
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
