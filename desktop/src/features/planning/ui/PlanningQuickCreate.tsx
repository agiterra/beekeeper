import * as React from "react";

import { AssigneePicker } from "@/features/project-todos/ui/AssigneePicker";
import type { TodoPerson } from "@/features/project-todos/lib/todoPeople";
import type { ProjectContainer } from "@/features/projects-container/lib/projectContainerModel";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";
import { Input } from "@/shared/ui/input";
import { Switch } from "@/shared/ui/switch";
import { Textarea } from "@/shared/ui/textarea";

import { DEFAULT_NATIVE_CONTAINER_TITLE } from "../lib/nativePlanning";
import type {
  PlanningCapabilities,
  PlanningContainer,
  PlanningCreateInput,
} from "../lib/planningTypes";
import { PriorityPicker, StatusPicker } from "./PlanningPickers";

function utf8Length(value: string): number {
  return new TextEncoder().encode(value).length;
}

export type QuickCreateDefaults = {
  statusId: string;
  priorityId: string;
  assignee: string | null;
  containerId: string | null;
};

/**
 * "New issue": a title, then whatever else is known. Enter in the title or
 * ⌘↵ anywhere creates; "Create more" keeps the dialog open for the next one,
 * the way a person files several issues in a row. Defaults come from where
 * it was opened — a status column, a priority group, an assignee.
 */
export function PlanningQuickCreate({
  open,
  project,
  capabilities,
  containers,
  defaults,
  personFor,
  onOpenChange,
  onCreate,
}: {
  open: boolean;
  project: ProjectContainer;
  capabilities: PlanningCapabilities;
  containers: readonly PlanningContainer[];
  defaults: QuickCreateDefaults;
  personFor: (pubkey: string) => TodoPerson;
  onOpenChange: (open: boolean) => void;
  /** Resolves once the issue exists; rejects if it was not created at all. */
  onCreate: (input: PlanningCreateInput) => Promise<void>;
}) {
  const [title, setTitle] = React.useState("");
  const [description, setDescription] = React.useState("");
  const [statusId, setStatusId] = React.useState(defaults.statusId);
  const [priorityId, setPriorityId] = React.useState(defaults.priorityId);
  const [assignee, setAssignee] = React.useState(defaults.assignee);
  const [containerId, setContainerId] = React.useState<string | null>(null);
  const [createMore, setCreateMore] = React.useState(false);
  const [creating, setCreating] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const titleRef = React.useRef<HTMLInputElement | null>(null);

  // Reset only when the dialog opens: a live update to the project's lists
  // while it is open must never wipe what the viewer is typing.
  const latest = React.useRef({ defaults, containers });
  latest.current = { defaults, containers };
  React.useEffect(() => {
    if (!open) return;
    const { defaults: initial, containers: lists } = latest.current;
    setTitle("");
    setDescription("");
    setStatusId(initial.statusId);
    setPriorityId(initial.priorityId);
    setAssignee(initial.assignee);
    setContainerId(initial.containerId ?? lists[0]?.id ?? null);
    setError(null);
  }, [open]);

  const trimmed = title.trim();
  const titleBytes = utf8Length(trimmed);
  const descriptionBytes = utf8Length(description.trim());
  const titleOver = titleBytes > capabilities.limits.titleBytes;
  const descriptionOver =
    descriptionBytes > capabilities.limits.descriptionBytes;
  const canSubmit =
    trimmed.length > 0 && !titleOver && !descriptionOver && !creating;
  const status =
    capabilities.statuses.find((s) => s.id === statusId) ??
    capabilities.statuses[0];
  const priority =
    capabilities.priorities.find((p) => p.id === priorityId) ??
    capabilities.priorities.at(-1);
  // After a first create made the default list, later creates go into it.
  const container =
    containers.find((c) => c.id === containerId) ?? containers[0] ?? null;

  const submit = async () => {
    if (!canSubmit) return;
    setCreating(true);
    setError(null);
    try {
      await onCreate({
        title: trimmed,
        description: description.trim().length > 0 ? description.trim() : null,
        statusId,
        priorityId,
        assignee,
        containerId: container?.id ?? null,
      });
      if (createMore) {
        setTitle("");
        setDescription("");
        titleRef.current?.focus();
      } else {
        onOpenChange(false);
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setCreating(false);
    }
  };

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent
        className="max-w-xl"
        data-testid="planning-quick-create"
        onKeyDown={(event) => {
          if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
            event.preventDefault();
            void submit();
          }
        }}
      >
        <form
          className="flex flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            void submit();
          }}
        >
          <DialogHeader>
            <DialogTitle>New issue</DialogTitle>
            <DialogDescription>
              {container
                ? `In ${project.name} · ${container.title}`
                : capabilities.createsDefaultContainer
                  ? `In ${project.name}. This project has no shared list yet, so this also creates one named “${DEFAULT_NATIVE_CONTAINER_TITLE}”.`
                  : `In ${project.name}`}
            </DialogDescription>
          </DialogHeader>
          <Input
            aria-invalid={titleOver || undefined}
            aria-label="Issue title"
            autoFocus
            data-testid="planning-quick-create-title"
            disabled={creating}
            onChange={(event) => setTitle(event.target.value)}
            placeholder="Issue title"
            ref={titleRef}
            value={title}
          />
          <Textarea
            aria-invalid={descriptionOver || undefined}
            aria-label="Description"
            className="min-h-24 text-sm"
            data-testid="planning-quick-create-description"
            disabled={creating}
            onChange={(event) => setDescription(event.target.value)}
            placeholder="Add a description (optional)"
            value={description}
          />
          {titleOver || descriptionOver ? (
            <p className="text-xs text-destructive" role="alert">
              {titleOver
                ? `A title holds at most ${capabilities.limits.titleBytes} bytes (${titleBytes}).`
                : `A description holds at most ${capabilities.limits.descriptionBytes} bytes (${descriptionBytes}).`}
            </p>
          ) : null}
          <div className="flex flex-wrap items-center gap-1">
            {status ? (
              <StatusPicker
                canEdit
                onChange={setStatusId}
                showLabel
                status={status}
                statuses={capabilities.statuses}
                testId="planning-quick-create-status"
              />
            ) : null}
            {priority ? (
              <PriorityPicker
                canEdit
                onChange={setPriorityId}
                priorities={capabilities.priorities}
                priority={priority}
                showLabel
                testId="planning-quick-create-priority"
              />
            ) : null}
            <AssigneePicker
              assignee={assignee ? personFor(assignee) : null}
              canEdit
              onChange={setAssignee}
              project={project}
              testId="planning-quick-create-assignee"
            />
            {containers.length > 1 ? (
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <button
                    className="inline-flex max-w-48 items-center rounded-sm px-1 py-0.5 text-xs text-foreground hover:bg-accent focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring"
                    data-testid="planning-quick-create-container"
                    type="button"
                  >
                    <span className="truncate">
                      List: {container?.title ?? "—"}
                    </span>
                  </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="start" className="w-56">
                  <DropdownMenuRadioGroup
                    onValueChange={setContainerId}
                    value={container?.id ?? ""}
                  >
                    {containers.map((option) => (
                      <DropdownMenuRadioItem key={option.id} value={option.id}>
                        <span className="truncate">{option.title}</span>
                      </DropdownMenuRadioItem>
                    ))}
                  </DropdownMenuRadioGroup>
                </DropdownMenuContent>
              </DropdownMenu>
            ) : null}
          </div>
          {error ? (
            <p
              className="text-xs text-destructive"
              data-testid="planning-quick-create-error"
              role="alert"
            >
              {error}
            </p>
          ) : null}
          <DialogFooter className="items-center gap-2 sm:justify-between">
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              <Switch
                aria-labelledby="planning-create-more-label"
                checked={createMore}
                data-testid="planning-quick-create-more"
                onCheckedChange={setCreateMore}
              />
              <span id="planning-create-more-label">Create more</span>
            </div>
            <div className="flex items-center gap-2">
              <Button
                onClick={() => onOpenChange(false)}
                type="button"
                variant="ghost"
              >
                Cancel
              </Button>
              <Button
                aria-keyshortcuts="Meta+Enter"
                data-testid="planning-quick-create-submit"
                disabled={!canSubmit}
                title="Create issue (⌘↵)"
                type="submit"
              >
                {creating ? "Creating…" : "Create issue"}
              </Button>
            </div>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
