import { Archive, ArchiveRestore, Pencil, Plus } from "lucide-react";
import * as React from "react";

import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";

import type { TodoList } from "../lib/todoFold";
import { todoTextError } from "../lib/todoOp";

/**
 * The rail of a project's lists: pick one, make one, rename or archive the
 * selected one, and show or hide the archived ones. All edits are gated on
 * `canEdit`.
 */
export function TodoListPicker({
  lists,
  selectedId,
  showArchived,
  canEdit,
  onSelect,
  onToggleArchived,
  onCreate,
  onRename,
  onSetArchived,
}: {
  lists: readonly TodoList[];
  selectedId: string | null;
  showArchived: boolean;
  canEdit: boolean;
  onSelect: (listId: string) => void;
  onToggleArchived: () => void;
  onCreate: (title: string) => void;
  onRename: (listId: string, title: string) => void;
  onSetArchived: (listId: string, archived: boolean) => void;
}) {
  const [mode, setMode] = React.useState<"idle" | "create" | "rename">("idle");
  const [draft, setDraft] = React.useState("");
  const inputRef = React.useRef<HTMLInputElement>(null);
  React.useEffect(() => {
    if (mode !== "idle") inputRef.current?.focus();
  }, [mode]);

  const selected = lists.find((list) => list.id === selectedId) ?? null;
  const visible = lists.filter((list) => showArchived || !list.archived);
  const archivedCount = lists.filter((list) => list.archived).length;
  const draftError =
    draft.trim().length > 0 ? todoTextError("title", draft.trim()) : null;

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    const title = draft.trim();
    if (title.length === 0 || draftError) return;
    if (mode === "create") onCreate(title);
    else if (mode === "rename" && selected) onRename(selected.id, title);
    setMode("idle");
    setDraft("");
  };

  return (
    <aside
      className="flex w-56 shrink-0 flex-col gap-2 border-border border-r pr-3"
      data-testid="todo-list-picker"
    >
      <div className="flex items-center gap-1">
        <span className="text-2xs font-medium uppercase tracking-wide text-muted-foreground">
          Lists
        </span>
        <span className="flex-1" />
        {canEdit ? (
          <Button
            aria-label="New list"
            data-testid="todo-list-new"
            onClick={() => {
              setDraft("");
              setMode("create");
            }}
            size="icon-xs"
            variant="ghost"
          >
            <Plus aria-hidden="true" />
          </Button>
        ) : null}
      </div>

      {mode !== "idle" ? (
        <form className="flex flex-col gap-1" onSubmit={submit}>
          <input
            aria-label={mode === "create" ? "New list title" : "List title"}
            className="h-7 rounded-md border border-input bg-background px-2 text-sm"
            data-testid="todo-list-title-input"
            maxLength={1024}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                setMode("idle");
                setDraft("");
              }
            }}
            placeholder={mode === "create" ? "List title" : undefined}
            ref={inputRef}
            value={draft}
          />
          {draftError ? (
            <p className="text-2xs text-destructive">{draftError}</p>
          ) : null}
          <div className="flex justify-end gap-1">
            <Button
              onClick={() => {
                setMode("idle");
                setDraft("");
              }}
              size="xs"
              type="button"
              variant="ghost"
            >
              Cancel
            </Button>
            <Button
              data-testid="todo-list-title-submit"
              disabled={draft.trim().length === 0 || draftError !== null}
              size="xs"
              type="submit"
            >
              {mode === "create" ? "Create" : "Rename"}
            </Button>
          </div>
        </form>
      ) : null}

      <ul className="flex flex-col gap-0.5">
        {visible.map((list) => (
          <li key={list.id}>
            <button
              aria-current={list.id === selectedId ? "true" : undefined}
              className={cn(
                "flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-sm hover:bg-muted/60",
                list.id === selectedId && "bg-muted text-foreground",
                list.archived && "text-muted-foreground",
              )}
              data-testid={`todo-list-${list.id.slice(0, 8)}`}
              onClick={() => onSelect(list.id)}
              type="button"
            >
              <span
                className={cn(
                  "min-w-0 flex-1 truncate",
                  list.archived && "italic",
                )}
              >
                {list.title}
              </span>
              <span className="text-2xs text-muted-foreground">
                {list.open.length}
              </span>
            </button>
          </li>
        ))}
        {visible.length === 0 ? (
          <li className="px-2 text-sm text-muted-foreground">No lists yet.</li>
        ) : null}
      </ul>

      {archivedCount > 0 ? (
        <button
          className="self-start px-2 text-2xs text-muted-foreground hover:text-foreground"
          data-testid="todo-list-toggle-archived"
          onClick={onToggleArchived}
          type="button"
        >
          {showArchived ? "Hide archived" : `Show ${archivedCount} archived`}
        </button>
      ) : null}

      {selected && canEdit ? (
        <div className="mt-auto flex items-center gap-1 border-border border-t pt-2">
          <Button
            aria-label="Rename list"
            data-testid="todo-list-rename"
            onClick={() => {
              setDraft(selected.title);
              setMode("rename");
            }}
            size="icon-xs"
            variant="ghost"
          >
            <Pencil aria-hidden="true" />
          </Button>
          <Button
            aria-label={selected.archived ? "Restore list" : "Archive list"}
            data-testid="todo-list-archive"
            onClick={() => onSetArchived(selected.id, !selected.archived)}
            size="icon-xs"
            variant="ghost"
          >
            {selected.archived ? (
              <ArchiveRestore aria-hidden="true" />
            ) : (
              <Archive aria-hidden="true" />
            )}
          </Button>
        </div>
      ) : null}
    </aside>
  );
}
