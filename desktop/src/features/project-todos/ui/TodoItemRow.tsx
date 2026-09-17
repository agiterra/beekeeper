import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { GripVertical, Trash2 } from "lucide-react";
import * as React from "react";

import type { ProjectContainer } from "@/features/projects-container/lib/projectContainerModel";
import { cn } from "@/shared/lib/cn";
import { Checkbox } from "@/shared/ui/checkbox";

import type { TodoItem } from "../lib/todoFold";
import { todoTextError } from "../lib/todoOp";
import type { TodoPerson } from "../lib/todoPeople";
import { AssigneePicker } from "./AssigneePicker";
import { DueDatePicker } from "./DueDatePicker";

export type TodoRowActions = {
  setDone: (item: TodoItem, done: boolean) => void;
  setText: (item: TodoItem, text: string) => void;
  setAssignee: (item: TodoItem, assignee: string | null) => void;
  setDue: (item: TodoItem, due: string | null) => void;
  remove: (item: TodoItem) => void;
};

/**
 * One to-do row: checkbox, click-to-edit text, assignee and due chips, and
 * remove. `sortable` adds the drag handle for the open list; the completed
 * list renders the same row without one.
 */
export function TodoItemRow({
  item,
  project,
  assignee,
  canEdit,
  sortable,
  actions,
}: {
  item: TodoItem;
  project: ProjectContainer;
  assignee: TodoPerson | null;
  canEdit: boolean;
  sortable: boolean;
  actions: TodoRowActions;
}) {
  const [editing, setEditing] = React.useState(false);
  const [draft, setDraft] = React.useState(item.text);
  const inputRef = React.useRef<HTMLInputElement>(null);
  React.useEffect(() => {
    if (editing) inputRef.current?.focus();
  }, [editing]);

  const commit = () => {
    setEditing(false);
    const next = draft.trim();
    if (next === item.text || todoTextError("text", next)) {
      setDraft(item.text);
      return;
    }
    actions.setText(item, next);
  };

  return (
    <div
      className={cn(
        "group flex items-start gap-2 rounded-md px-2 py-1.5 hover:bg-muted/60",
        item.done && "text-muted-foreground",
      )}
      data-testid={`todo-item-${item.id.slice(0, 8)}`}
    >
      {sortable && canEdit ? (
        <DragHandle itemId={item.id} />
      ) : (
        <span className="w-4 shrink-0" />
      )}
      <Checkbox
        aria-label={item.done ? "Mark not done" : "Mark done"}
        checked={item.done}
        className="mt-0.5"
        data-testid="todo-item-checkbox"
        disabled={!canEdit}
        onCheckedChange={(checked) => actions.setDone(item, checked === true)}
      />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        {editing ? (
          <input
            className="h-6 w-full rounded-sm border border-input bg-background px-1 text-sm"
            data-testid="todo-item-text-input"
            maxLength={1024}
            onBlur={commit}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                commit();
              } else if (event.key === "Escape") {
                setDraft(item.text);
                setEditing(false);
              }
            }}
            ref={inputRef}
            value={draft}
          />
        ) : (
          <button
            className={cn(
              "min-w-0 break-words text-left text-sm",
              item.done && "line-through",
              canEdit && "cursor-text",
            )}
            data-testid="todo-item-text"
            disabled={!canEdit}
            onClick={() => {
              if (!canEdit) return;
              setDraft(item.text);
              setEditing(true);
            }}
            type="button"
          >
            {item.text}
          </button>
        )}
        <div className="flex flex-wrap items-center gap-1">
          <AssigneePicker
            assignee={assignee}
            canEdit={canEdit}
            onChange={(pubkey) => actions.setAssignee(item, pubkey)}
            project={project}
            testId="todo-item-assignee"
          />
          <DueDatePicker
            canEdit={canEdit}
            done={item.done}
            due={item.due}
            onChange={(due) => actions.setDue(item, due)}
            testId="todo-item-due"
          />
        </div>
      </div>
      {canEdit ? (
        <button
          aria-label="Remove item"
          className="invisible mt-0.5 rounded-sm p-0.5 text-muted-foreground hover:text-destructive group-hover:visible focus-visible:visible"
          data-testid="todo-item-remove"
          onClick={() => actions.remove(item)}
          type="button"
        >
          <Trash2 aria-hidden="true" className="h-3.5 w-3.5" />
        </button>
      ) : null}
    </div>
  );
}

function DragHandle({ itemId }: { itemId: string }) {
  const { attributes, listeners, setActivatorNodeRef } = useSortable({
    id: itemId,
  });
  return (
    <button
      aria-label="Drag to reorder"
      className="mt-0.5 cursor-grab touch-none text-muted-foreground/60 hover:text-muted-foreground active:cursor-grabbing"
      data-testid="todo-item-drag-handle"
      ref={setActivatorNodeRef}
      type="button"
      {...attributes}
      {...listeners}
    >
      <GripVertical aria-hidden="true" className="h-4 w-4" />
    </button>
  );
}

/** The sortable shell around a row in the open list. */
export function SortableTodoRow({
  itemId,
  children,
}: {
  itemId: string;
  children: React.ReactNode;
}) {
  const { setNodeRef, transform, transition, isDragging } = useSortable({
    id: itemId,
  });
  const style: React.CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
  };
  return (
    <div
      className={cn(isDragging && "opacity-60")}
      ref={setNodeRef}
      style={style}
    >
      {children}
    </div>
  );
}
