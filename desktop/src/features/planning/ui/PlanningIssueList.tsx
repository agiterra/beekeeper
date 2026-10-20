import { Plus } from "lucide-react";
import * as React from "react";

import { AssigneePicker } from "@/features/project-todos/ui/AssigneePicker";
import type { TodoPerson } from "@/features/project-todos/lib/todoPeople";
import { formatDue, isOverdue } from "@/features/project-todos/lib/todoPeople";
import type { ProjectContainer } from "@/features/projects-container/lib/projectContainerModel";
import { cn } from "@/shared/lib/cn";

import type {
  PlanningCapabilities,
  PlanningContainer,
  PlanningIssue,
} from "../lib/planningTypes";
import { type PlanningGroup, isClosedIssue } from "../lib/planningView";
import {
  PendingMark,
  PriorityPicker,
  StatusGlyph,
  StatusPicker,
} from "./PlanningPickers";

export type PlanningRowActions = {
  open: (issue: PlanningIssue) => void;
  select: (issue: PlanningIssue) => void;
  setStatus: (issue: PlanningIssue, statusId: string) => void;
  setPriority: (issue: PlanningIssue, priorityId: string) => void;
  setAssignee: (issue: PlanningIssue, assignee: string | null) => void;
  createIn: (group: PlanningGroup) => void;
};

export type PlanningListProps = {
  project: ProjectContainer;
  groups: readonly PlanningGroup[];
  capabilities: PlanningCapabilities;
  containers: readonly PlanningContainer[];
  showContainer: boolean;
  canEdit: boolean;
  selectedKey: string | null;
  openKey: string | null;
  isPending: (key: string) => boolean;
  personFor: (pubkey: string) => TodoPerson;
  actions: PlanningRowActions;
};

/** The group header: glyph, name, count, and "new issue here". */
export function PlanningGroupHeader({
  group,
  canEdit,
  onCreate,
  compact = false,
}: {
  group: PlanningGroup;
  canEdit: boolean;
  onCreate: () => void;
  compact?: boolean;
}) {
  return (
    <div
      className={cn(
        "flex items-center gap-2 text-xs font-medium text-foreground",
        compact ? "px-1 py-1" : "bg-muted/40 px-3 py-1.5",
      )}
      data-testid={`planning-group-${group.id}`}
    >
      {group.category ? <StatusGlyph category={group.category} /> : null}
      <span className="truncate">{group.label}</span>
      <span className="text-muted-foreground tabular-nums">
        {group.issues.length}
      </span>
      <span className="flex-1" />
      {canEdit ? (
        <button
          aria-label={`New issue in ${group.label}`}
          className="rounded-sm p-0.5 text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring"
          data-testid={`planning-group-create-${group.id}`}
          onClick={onCreate}
          type="button"
        >
          <Plus aria-hidden="true" className="h-3.5 w-3.5" />
        </button>
      ) : null}
    </div>
  );
}

function PlanningRow({
  project,
  issue,
  capabilities,
  containerTitle,
  canEdit,
  selected,
  open,
  pending,
  assignee,
  actions,
}: {
  project: ProjectContainer;
  issue: PlanningIssue;
  capabilities: PlanningCapabilities;
  containerTitle: string | null;
  canEdit: boolean;
  selected: boolean;
  open: boolean;
  pending: boolean;
  assignee: TodoPerson | null;
  actions: PlanningRowActions;
}) {
  const closed = isClosedIssue(issue);
  return (
    <li
      aria-current={open ? "true" : undefined}
      className={cn(
        "group flex h-9 items-center gap-1 border-b border-border/60 px-2 text-sm",
        selected ? "bg-accent/70" : "hover:bg-accent/40",
      )}
      data-selected={selected ? "true" : undefined}
      data-testid={`planning-row-${issue.issueId}`}
    >
      <PriorityPicker
        canEdit={canEdit}
        onChange={(id) => actions.setPriority(issue, id)}
        priorities={capabilities.priorities}
        priority={issue.priority}
        testId={`planning-row-priority-${issue.issueId}`}
      />
      <StatusPicker
        canEdit={canEdit}
        onChange={(id) => actions.setStatus(issue, id)}
        status={issue.status}
        statuses={capabilities.statuses}
        testId={`planning-row-status-${issue.issueId}`}
      />
      <button
        className={cn(
          "min-w-0 flex-1 truncate rounded-sm px-1 py-0.5 text-left focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring",
          closed ? "text-muted-foreground" : "text-foreground",
        )}
        data-planning-row-title={issue.key}
        data-testid={`planning-row-title-${issue.issueId}`}
        onClick={() => actions.open(issue)}
        onFocus={() => actions.select(issue)}
        type="button"
      >
        {issue.title}
      </button>
      {pending ? <PendingMark /> : null}
      {containerTitle ? (
        <span className="hidden max-w-32 truncate rounded-sm border border-border px-1.5 text-2xs text-muted-foreground sm:inline">
          {containerTitle}
        </span>
      ) : null}
      {issue.due ? (
        <span
          className={cn(
            "text-2xs tabular-nums",
            !closed && isOverdue(issue.due)
              ? "text-red-600"
              : "text-muted-foreground",
          )}
        >
          {formatDue(issue.due)}
        </span>
      ) : null}
      <AssigneePicker
        assignee={assignee}
        canEdit={canEdit}
        onChange={(pubkey) => actions.setAssignee(issue, pubkey)}
        project={project}
        testId={`planning-row-assignee-${issue.issueId}`}
      />
    </li>
  );
}

/**
 * The compact list: one row per issue, grouped. Every control on a row edits
 * the issue in place; the title opens the detail. Empty groups are folded
 * away in the list (the board keeps them as drop targets).
 */
export function PlanningIssueList({
  project,
  groups,
  capabilities,
  containers,
  showContainer,
  canEdit,
  selectedKey,
  openKey,
  isPending,
  personFor,
  actions,
}: PlanningListProps) {
  const titles = React.useMemo(
    () => new Map(containers.map((c) => [c.id, c.title])),
    [containers],
  );
  const visible = groups.filter((group) => group.issues.length > 0);
  return (
    <div className="flex flex-col" data-testid="planning-list">
      {visible.map((group) => (
        <section aria-label={group.label} key={group.id}>
          <PlanningGroupHeader
            canEdit={canEdit}
            group={group}
            onCreate={() => actions.createIn(group)}
          />
          <ul>
            {group.issues.map((issue) => (
              <PlanningRow
                actions={actions}
                assignee={issue.assignee ? personFor(issue.assignee) : null}
                canEdit={canEdit}
                capabilities={capabilities}
                containerTitle={
                  showContainer ? (titles.get(issue.containerId) ?? null) : null
                }
                issue={issue}
                key={issue.key}
                open={openKey === issue.key}
                pending={isPending(issue.key)}
                project={project}
                selected={selectedKey === issue.key}
              />
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}
