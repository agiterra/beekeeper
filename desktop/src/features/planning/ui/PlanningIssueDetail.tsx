import { MessageSquareOff, X } from "lucide-react";
import * as React from "react";

import {
  AssigneePicker,
  PersonAvatar,
} from "@/features/project-todos/ui/AssigneePicker";
import type { TodoPerson } from "@/features/project-todos/lib/todoPeople";
import { formatDue } from "@/features/project-todos/lib/todoPeople";
import type { ProjectContainer } from "@/features/projects-container/lib/projectContainerModel";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Textarea } from "@/shared/ui/textarea";

import type { ActivityEntry } from "../lib/nativeActivity";
import type { PlanningCapabilities, PlanningIssue } from "../lib/planningTypes";
import { PendingMark, PriorityPicker, StatusPicker } from "./PlanningPickers";

function utf8Length(value: string): number {
  return new TextEncoder().encode(value).length;
}

function formatWhen(secs: number): string {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(new Date(secs * 1_000));
}

export type PlanningDetailActions = {
  close: () => void;
  setTitle: (issue: PlanningIssue, title: string) => Promise<void>;
  setDescription: (
    issue: PlanningIssue,
    description: string | null,
  ) => Promise<void>;
  setStatus: (issue: PlanningIssue, statusId: string) => void;
  setPriority: (issue: PlanningIssue, priorityId: string) => void;
  setAssignee: (issue: PlanningIssue, assignee: string | null) => void;
};

function TitleField({
  issue,
  canEdit,
  limit,
  onSave,
}: {
  issue: PlanningIssue;
  canEdit: boolean;
  limit: number;
  onSave: (title: string) => Promise<void>;
}) {
  const [draft, setDraft] = React.useState(issue.title);
  const [error, setError] = React.useState<string | null>(null);
  const [saving, setSaving] = React.useState(false);
  // A new remote title replaces the draft unless the viewer is mid-edit.
  const editing = React.useRef(false);
  // Escape blurs the field to leave it; that blur must not save. The flag
  // is read by the blur handler the same tick, before any re-render could
  // hand it a fresh closure, so the restored draft never races the typed one.
  const discardOnBlur = React.useRef(false);
  React.useEffect(() => {
    if (!editing.current) setDraft(issue.title);
  }, [issue.title]);

  if (!canEdit) {
    return (
      <h2
        className="break-words text-lg font-semibold text-foreground"
        data-testid="planning-detail-title"
      >
        {issue.title}
      </h2>
    );
  }
  const commit = async () => {
    editing.current = false;
    const next = draft.trim();
    if (next === issue.title) return;
    if (next.length === 0) {
      setDraft(issue.title);
      return;
    }
    if (utf8Length(next) > limit) {
      setError(`A title holds at most ${limit} bytes.`);
      return;
    }
    setSaving(true);
    setError(null);
    try {
      await onSave(next);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setSaving(false);
    }
  };
  return (
    <div className="flex flex-col gap-1">
      <Input
        aria-label="Title"
        className="h-auto border-transparent px-1 py-1 text-lg font-semibold shadow-none hover:border-input focus-visible:border-input"
        data-testid="planning-detail-title"
        disabled={saving}
        onBlur={() => {
          if (discardOnBlur.current) {
            discardOnBlur.current = false;
            return;
          }
          void commit();
        }}
        onChange={(event) => {
          editing.current = true;
          setDraft(event.target.value);
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            event.currentTarget.blur();
          } else if (event.key === "Escape") {
            event.preventDefault();
            event.stopPropagation();
            editing.current = false;
            discardOnBlur.current = true;
            setDraft(issue.title);
            setError(null);
            event.currentTarget.blur();
          }
        }}
        value={draft}
      />
      {saving ? <PendingMark /> : null}
      {error ? (
        <p className="text-xs text-destructive" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}

function DescriptionField({
  issue,
  canEdit,
  limit,
  onSave,
}: {
  issue: PlanningIssue;
  canEdit: boolean;
  limit: number;
  onSave: (description: string | null) => Promise<void>;
}) {
  const [editing, setEditing] = React.useState(false);
  const [draft, setDraft] = React.useState(issue.description ?? "");
  const [error, setError] = React.useState<string | null>(null);
  const [saving, setSaving] = React.useState(false);
  React.useEffect(() => {
    if (!editing) setDraft(issue.description ?? "");
  }, [editing, issue.description]);
  const bytes = utf8Length(draft.trim());
  const over = bytes > limit;

  const save = async () => {
    const next = draft.trim();
    const value = next.length === 0 ? null : next;
    if (value === issue.description) {
      setEditing(false);
      return;
    }
    if (over) return;
    setSaving(true);
    setError(null);
    try {
      await onSave(value);
      setEditing(false);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setSaving(false);
    }
  };

  if (!editing) {
    return (
      <div className="flex flex-col gap-1">
        {issue.description ? (
          <p
            className="whitespace-pre-wrap break-words text-sm text-foreground"
            data-testid="planning-detail-description"
          >
            {issue.description}
          </p>
        ) : (
          <p
            className="text-sm text-muted-foreground"
            data-testid="planning-detail-description-empty"
          >
            No description.
          </p>
        )}
        {canEdit ? (
          <div>
            <Button
              data-testid="planning-detail-edit-description"
              onClick={() => setEditing(true)}
              size="xs"
              variant="ghost"
            >
              {issue.description ? "Edit description" : "Add description"}
            </Button>
          </div>
        ) : null}
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-1.5">
      <Textarea
        aria-label="Description"
        autoFocus
        className="min-h-32 text-sm"
        data-testid="planning-detail-description-input"
        disabled={saving}
        onChange={(event) => setDraft(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
            event.preventDefault();
            void save();
          } else if (event.key === "Escape") {
            event.preventDefault();
            event.stopPropagation();
            setEditing(false);
          }
        }}
        value={draft}
      />
      <div className="flex items-center gap-2">
        <span
          className={
            over
              ? "text-2xs text-destructive"
              : "text-2xs text-muted-foreground"
          }
        >
          {bytes} / {limit} bytes
        </span>
        <span className="flex-1" />
        <Button onClick={() => setEditing(false)} size="xs" variant="ghost">
          Cancel
        </Button>
        <Button
          data-testid="planning-detail-save-description"
          disabled={saving || over}
          onClick={() => void save()}
          size="xs"
          title="Save (⌘↵)"
        >
          {saving ? "Saving…" : "Save"}
        </Button>
      </div>
      {error ? (
        <p className="text-xs text-destructive" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}

function Property({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex min-h-7 items-center gap-2">
      <span className="w-20 shrink-0 text-xs text-muted-foreground">
        {label}
      </span>
      <div className="min-w-0 flex-1">{children}</div>
    </div>
  );
}

/**
 * One issue: its title and description, its properties, and its history.
 * Every field saves as its own write; a field whose write is in flight says
 * so, and one that fails keeps the viewer's draft beside the error.
 */
export function PlanningIssueDetail({
  project,
  issue,
  containerTitle,
  capabilities,
  canEdit,
  readOnlyReason,
  pendingFields,
  personFor,
  activity,
  actions,
}: {
  project: ProjectContainer;
  issue: PlanningIssue;
  containerTitle: string | null;
  capabilities: PlanningCapabilities;
  canEdit: boolean;
  readOnlyReason: string | null;
  pendingFields: ReadonlySet<string>;
  personFor: (pubkey: string) => TodoPerson;
  activity: readonly ActivityEntry[];
  actions: PlanningDetailActions;
}) {
  const assignee = issue.assignee ? personFor(issue.assignee) : null;
  const creator = personFor(issue.createdBy);
  return (
    <aside
      aria-label={`Issue: ${issue.title}`}
      className="flex w-[26rem] shrink-0 flex-col gap-4 overflow-y-auto border-l border-border bg-background px-4 py-3"
      data-testid="planning-detail"
    >
      <div className="flex items-center gap-2">
        <span className="min-w-0 truncate text-xs text-muted-foreground">
          {containerTitle ?? "List"}
        </span>
        <span className="flex-1" />
        <button
          aria-keyshortcuts="Escape"
          aria-label="Close issue"
          className="rounded-sm p-1 text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring"
          data-testid="planning-detail-close"
          onClick={actions.close}
          type="button"
        >
          <X aria-hidden="true" className="h-4 w-4" />
        </button>
      </div>
      {readOnlyReason ? (
        <p className="rounded-md border border-border bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
          Read-only: {readOnlyReason}
        </p>
      ) : null}
      <TitleField
        canEdit={canEdit}
        issue={issue}
        key={`title-${issue.key}`}
        limit={capabilities.limits.titleBytes}
        onSave={(title) => actions.setTitle(issue, title)}
      />
      <div
        className="flex flex-col gap-0.5"
        data-testid="planning-detail-properties"
      >
        <Property label="Status">
          <StatusPicker
            canEdit={canEdit}
            onChange={(id) => actions.setStatus(issue, id)}
            pending={pendingFields.has("status")}
            showLabel
            status={issue.status}
            statuses={capabilities.statuses}
            testId="planning-detail-status"
          />
        </Property>
        <Property label="Priority">
          <PriorityPicker
            canEdit={canEdit}
            onChange={(id) => actions.setPriority(issue, id)}
            pending={pendingFields.has("priority")}
            priorities={capabilities.priorities}
            priority={issue.priority}
            showLabel
            testId="planning-detail-priority"
          />
        </Property>
        <Property label="Assignee">
          <div className="flex items-center gap-1">
            <AssigneePicker
              assignee={assignee}
              canEdit={canEdit}
              onChange={(pubkey) => actions.setAssignee(issue, pubkey)}
              project={project}
              testId="planning-detail-assignee"
            />
            {pendingFields.has("assignee") ? <PendingMark /> : null}
          </div>
        </Property>
        {issue.due ? (
          <Property label="Due">
            <span className="px-1 text-xs text-foreground">
              {formatDue(issue.due)}
            </span>
          </Property>
        ) : null}
        <Property label="Created">
          <span className="flex items-center gap-1 px-1 text-xs text-foreground">
            <PersonAvatar person={creator} />
            <span className="truncate">{creator.name}</span>
            <span className="text-muted-foreground">
              · {formatWhen(issue.createdAt)}
            </span>
          </span>
        </Property>
      </div>
      <section aria-label="Description" className="flex flex-col gap-1">
        <h3 className="text-xs font-medium text-muted-foreground">
          Description
        </h3>
        <DescriptionField
          canEdit={canEdit}
          issue={issue}
          key={`description-${issue.key}`}
          limit={capabilities.limits.descriptionBytes}
          onSave={(description) => actions.setDescription(issue, description)}
        />
      </section>
      <section aria-label="Activity" className="flex flex-col gap-1.5">
        <h3 className="text-xs font-medium text-muted-foreground">Activity</h3>
        <ol
          className="flex flex-col gap-1"
          data-testid="planning-detail-activity"
        >
          {activity.map((entry) => {
            const actor = personFor(entry.actor);
            const subject =
              entry.subject != null ? personFor(entry.subject) : null;
            return (
              <li
                className="flex items-start gap-1.5 text-xs text-muted-foreground"
                key={entry.id}
              >
                <PersonAvatar person={actor} />
                <span className="min-w-0 flex-1">
                  <span className="text-foreground">{actor.name}</span>{" "}
                  {entry.summary}
                  {subject ? (
                    <>
                      {" "}
                      <span className="text-foreground">{subject.name}</span>
                    </>
                  ) : null}
                </span>
                <time
                  className="shrink-0 tabular-nums"
                  dateTime={new Date(entry.createdAt * 1_000).toISOString()}
                >
                  {formatWhen(entry.createdAt)}
                </time>
              </li>
            );
          })}
        </ol>
        {capabilities.comments ? null : (
          <p
            className="flex items-start gap-1.5 text-2xs text-muted-foreground"
            data-testid="planning-detail-no-comments"
          >
            <MessageSquareOff aria-hidden="true" className="mt-px h-3 w-3" />
            Comments are not available for Beekeeper issues yet. Discuss the
            issue in one of the project's channels.
          </p>
        )}
      </section>
    </aside>
  );
}
