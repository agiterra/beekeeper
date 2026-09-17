import {
  DndContext,
  type DragEndEvent,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  SortableContext,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { Plus } from "lucide-react";
import * as React from "react";

import type { ProjectContainer } from "@/features/projects-container/lib/projectContainerModel";
import { Button } from "@/shared/ui/button";

import type { TodoItem, TodoList } from "../lib/todoFold";
import { todoTextError } from "../lib/todoOp";
import type { TodoPerson } from "../lib/todoPeople";
import {
  SortableTodoRow,
  TodoItemRow,
  type TodoRowActions,
} from "./TodoItemRow";

export type TodoListPanelProps = {
  list: TodoList;
  project: ProjectContainer;
  canEdit: boolean;
  /** Resolve a pubkey to a person for the assignee chip. */
  personFor: (pubkey: string) => TodoPerson;
  onAdd: (text: string) => void;
  /** Move an open item to `index` among the open items (0 = first). */
  onMove: (item: TodoItem, index: number) => void;
  actions: TodoRowActions;
};

/**
 * One list: the add field, the open items as a drag-sortable list, then the
 * completed items most recent first. Every affordance is gated on `canEdit`,
 * so a viewer sees the list and not a control that would fail.
 */
export function TodoListPanel({
  list,
  project,
  canEdit,
  personFor,
  onAdd,
  onMove,
  actions,
}: TodoListPanelProps) {
  const [draft, setDraft] = React.useState("");
  const draftError =
    draft.trim().length > 0 ? todoTextError("text", draft.trim()) : null;
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
  );
  const openIds = React.useMemo(
    () => list.open.map((item) => item.id),
    [list.open],
  );

  const handleDragEnd = React.useCallback(
    (event: DragEndEvent) => {
      const { active, over } = event;
      if (!over) return;
      const oldIdx = openIds.indexOf(active.id as string);
      const newIdx = openIds.indexOf(over.id as string);
      if (oldIdx === -1 || newIdx === -1 || oldIdx === newIdx) return;
      const item = list.open[oldIdx];
      if (item) onMove(item, newIdx);
    },
    [list.open, onMove, openIds],
  );

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    const text = draft.trim();
    if (text.length === 0 || draftError) return;
    onAdd(text);
    setDraft("");
  };

  return (
    <div
      className="flex min-w-0 flex-1 flex-col gap-4"
      data-testid="todo-list-panel"
    >
      {canEdit ? (
        <form className="flex items-start gap-2" onSubmit={submit}>
          <input
            aria-label="New to-do"
            className="h-8 min-w-0 flex-1 rounded-md border border-input bg-background px-2 text-sm"
            data-testid="todo-add-input"
            maxLength={1024}
            onChange={(event) => setDraft(event.target.value)}
            placeholder="Add a to-do…"
            value={draft}
          />
          <Button
            data-testid="todo-add-submit"
            disabled={draft.trim().length === 0 || draftError !== null}
            size="sm"
            type="submit"
          >
            <Plus aria-hidden="true" className="mr-1 h-3.5 w-3.5" />
            Add
          </Button>
        </form>
      ) : null}
      {draftError ? (
        <p className="text-2xs text-destructive">{draftError}</p>
      ) : null}

      <section data-testid="todo-open-section">
        <h3 className="mb-1 text-2xs font-medium uppercase tracking-wide text-muted-foreground">
          Open · {list.open.length}
        </h3>
        {list.open.length === 0 ? (
          <p className="px-2 text-sm text-muted-foreground">
            {list.completed.length === 0 ? "Nothing here yet." : "All done."}
          </p>
        ) : (
          <DndContext
            collisionDetection={closestCenter}
            onDragEnd={handleDragEnd}
            sensors={sensors}
          >
            <SortableContext
              items={openIds}
              strategy={verticalListSortingStrategy}
            >
              <ul className="flex flex-col">
                {list.open.map((item) => (
                  <li key={item.id}>
                    <SortableTodoRow itemId={item.id}>
                      <TodoItemRow
                        actions={actions}
                        assignee={
                          item.assignee ? personFor(item.assignee) : null
                        }
                        canEdit={canEdit}
                        item={item}
                        project={project}
                        sortable
                      />
                    </SortableTodoRow>
                  </li>
                ))}
              </ul>
            </SortableContext>
          </DndContext>
        )}
      </section>

      {list.completed.length > 0 ? (
        <section data-testid="todo-completed-section">
          <h3 className="mb-1 text-2xs font-medium uppercase tracking-wide text-muted-foreground">
            Completed · {list.completed.length}
          </h3>
          <ul className="flex flex-col">
            {list.completed.map((item) => (
              <li key={item.id}>
                <TodoItemRow
                  actions={actions}
                  assignee={item.assignee ? personFor(item.assignee) : null}
                  canEdit={canEdit}
                  item={item}
                  project={project}
                  sortable={false}
                />
              </li>
            ))}
          </ul>
        </section>
      ) : null}
    </div>
  );
}
