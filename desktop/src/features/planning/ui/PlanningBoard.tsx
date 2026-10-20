import {
  DndContext,
  type DragEndEvent,
  PointerSensor,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import { CSS } from "@dnd-kit/utilities";
import type * as React from "react";

import { PersonAvatar } from "@/features/project-todos/ui/AssigneePicker";
import type { TodoPerson } from "@/features/project-todos/lib/todoPeople";
import { cn } from "@/shared/lib/cn";

import type { PlanningCapabilities, PlanningIssue } from "../lib/planningTypes";
import type { PlanningGroup, PlanningGroupBy } from "../lib/planningView";
import {
  PlanningGroupHeader,
  type PlanningRowActions,
} from "./PlanningIssueList";
import { PendingMark, PriorityPicker, StatusPicker } from "./PlanningPickers";

/** `true` when dropping onto a group of `groupBy` changes a field we can write. */
export function boardDropWrites(groupBy: PlanningGroupBy): boolean {
  return (
    groupBy === "status" || groupBy === "priority" || groupBy === "assignee"
  );
}

type CardProps = {
  issue: PlanningIssue;
  capabilities: PlanningCapabilities;
  canEdit: boolean;
  draggable: boolean;
  selected: boolean;
  pending: boolean;
  assignee: TodoPerson | null;
  actions: PlanningRowActions;
};

function PlanningCard({
  issue,
  capabilities,
  canEdit,
  draggable,
  selected,
  pending,
  assignee,
  actions,
}: CardProps) {
  const { attributes, listeners, setNodeRef, transform, isDragging } =
    useDraggable({ id: issue.key, disabled: !draggable });
  // The card is the drag surface; dnd-kit's own `role="button"` would make
  // a list item claim to be a button around the real title button.
  const { role: _role, tabIndex: _tabIndex, ...dragAttributes } = attributes;
  return (
    <li
      {...dragAttributes}
      {...listeners}
      className={cn(
        "rounded-md border border-border bg-background p-2 shadow-xs",
        selected && "ring-2 ring-ring",
        draggable && "cursor-grab",
        isDragging && "relative z-10 cursor-grabbing opacity-90 shadow-md",
      )}
      data-testid={`planning-card-${issue.issueId}`}
      ref={setNodeRef}
      style={{ transform: CSS.Translate.toString(transform) }}
    >
      <button
        className="block w-full rounded-sm text-left text-sm text-foreground focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring"
        data-planning-row-title={issue.key}
        data-testid={`planning-card-title-${issue.issueId}`}
        onClick={() => actions.open(issue)}
        onFocus={() => actions.select(issue)}
        type="button"
      >
        {issue.title}
      </button>
      <div className="mt-1.5 flex items-center gap-1">
        <StatusPicker
          canEdit={canEdit}
          onChange={(id) => actions.setStatus(issue, id)}
          status={issue.status}
          statuses={capabilities.statuses}
          testId={`planning-card-status-${issue.issueId}`}
        />
        <PriorityPicker
          canEdit={canEdit}
          onChange={(id) => actions.setPriority(issue, id)}
          priorities={capabilities.priorities}
          priority={issue.priority}
          testId={`planning-card-priority-${issue.issueId}`}
        />
        {pending ? <PendingMark /> : null}
        <span className="flex-1" />
        {assignee ? (
          <span title={assignee.name}>
            <PersonAvatar person={assignee} />
          </span>
        ) : null}
      </div>
    </li>
  );
}

function PlanningColumn({
  group,
  canEdit,
  onCreate,
  children,
}: {
  group: PlanningGroup;
  canEdit: boolean;
  onCreate: () => void;
  children: React.ReactNode;
}) {
  const { setNodeRef, isOver } = useDroppable({ id: group.id });
  return (
    <section
      aria-label={group.label}
      className={cn(
        "flex w-72 shrink-0 flex-col rounded-md border border-border bg-muted/20",
        isOver && "border-primary/60 bg-primary/5",
      )}
      data-testid={`planning-column-${group.id}`}
      ref={setNodeRef}
    >
      <div className="px-2 pt-1.5">
        <PlanningGroupHeader
          canEdit={canEdit}
          compact
          group={group}
          onCreate={onCreate}
        />
      </div>
      <ul className="flex min-h-16 flex-1 flex-col gap-1.5 overflow-y-auto p-2">
        {children}
      </ul>
    </section>
  );
}

/**
 * The board: the same filtered issues as the list, one column per group.
 * Dragging a card to another column writes that column's value (status,
 * priority or assignee) — the same single-field write the card's own menus
 * make, which is also the keyboard path. Grouping by list has nothing to
 * write on a drop, so cards do not drag there.
 */
export function PlanningBoard({
  groups,
  groupBy,
  capabilities,
  canEdit,
  selectedKey,
  isPending,
  personFor,
  actions,
  issueByKey,
}: {
  groups: readonly PlanningGroup[];
  groupBy: PlanningGroupBy;
  capabilities: PlanningCapabilities;
  canEdit: boolean;
  selectedKey: string | null;
  isPending: (key: string) => boolean;
  personFor: (pubkey: string) => TodoPerson;
  actions: PlanningRowActions;
  issueByKey: (key: string) => PlanningIssue | undefined;
}) {
  const draggable = canEdit && boardDropWrites(groupBy);
  // A few pixels of travel before a drag starts, so a click on the title or
  // a menu on the card is still a click.
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
  );

  const onDragEnd = (event: DragEndEvent) => {
    const issue = issueByKey(String(event.active.id));
    const group = groups.find((g) => g.id === event.over?.id);
    if (!issue || !group) return;
    if (groupBy === "status" && issue.status.id !== group.id) {
      actions.setStatus(issue, group.id);
    } else if (groupBy === "priority" && issue.priority.id !== group.id) {
      actions.setPriority(issue, group.id);
    } else if (groupBy === "assignee") {
      const assignee = group.assignee ?? null;
      if (issue.assignee !== assignee) actions.setAssignee(issue, assignee);
    }
  };

  return (
    <DndContext onDragEnd={onDragEnd} sensors={sensors}>
      <div
        className="flex min-h-0 flex-1 gap-3 overflow-x-auto pb-2"
        data-testid="planning-board"
      >
        {groups.map((group) => (
          <PlanningColumn
            canEdit={canEdit}
            group={group}
            key={group.id}
            onCreate={() => actions.createIn(group)}
          >
            {group.issues.map((issue) => (
              <PlanningCard
                actions={actions}
                assignee={issue.assignee ? personFor(issue.assignee) : null}
                canEdit={canEdit}
                capabilities={capabilities}
                draggable={draggable}
                issue={issue}
                key={issue.key}
                pending={isPending(issue.key)}
                selected={selectedKey === issue.key}
              />
            ))}
          </PlanningColumn>
        ))}
      </div>
    </DndContext>
  );
}
