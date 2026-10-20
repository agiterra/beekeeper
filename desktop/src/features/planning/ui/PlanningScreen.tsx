import * as React from "react";
import { useNavigate } from "@tanstack/react-router";
import { toast } from "sonner";

import { useUsersBatchQuery } from "@/features/profile/hooks";
import { todoWriteAccess } from "@/features/project-todos/lib/todoAccess";
import { todoPerson } from "@/features/project-todos/lib/todoPeople";
import { useProjectContainerQuery } from "@/features/projects-container/hooks";
import { LOCAL_GENERAL_ID } from "@/features/projects-container/lib/projectContainerModel";
import { useProjectRosterQuery } from "@/features/projects-container/lib/projectMembers";
import { useProjectCapabilities } from "@/features/projects-container/lib/projectPermissions";
import { ProjectPageTabs } from "@/features/projects-container/ui/ProjectPageTabs";
import { useIdentityQuery } from "@/shared/api/hooks";
import { useFeatureEnabled } from "@/shared/features";

import { NATIVE_CAPABILITIES } from "../lib/nativePlanning";
import { PlanningPartialWriteError } from "../lib/planningErrors";
import {
  planningDisplayKey,
  readPlanningDisplay,
  writePlanningDisplay,
} from "../lib/planningDisplayStorage";
import { resolvePlanningProvider } from "../lib/planningProviders";
import type {
  PlanningAccess,
  PlanningCreateInput,
  PlanningIssue,
} from "../lib/planningTypes";
import {
  DEFAULT_PLANNING_DISPLAY,
  DEFAULT_PLANNING_FILTERS,
  type PlanningDisplay,
  type PlanningFilters,
  type PlanningGroup,
  defaultCreateIds,
  personPubkey,
  stepSelection,
  viewerPersonId,
} from "../lib/planningView";
import { useNativePlanning } from "../lib/useNativePlanning";
import { PlanningIssueDetail } from "./PlanningIssueDetail";
import type { PlanningRowActions } from "./PlanningIssueList";
import {
  PlanningQuickCreate,
  type QuickCreateDefaults,
} from "./PlanningQuickCreate";
import { PlanningView } from "./PlanningView";

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** `true` when a keystroke belongs to a field, a menu or a dialog, not the page. */
function keyBelongsElsewhere(event: KeyboardEvent): boolean {
  if (event.defaultPrevented || event.metaKey || event.ctrlKey || event.altKey)
    return true;
  const target = event.target as HTMLElement | null;
  if (
    target &&
    (target.isContentEditable ||
      target.closest("input, textarea, select, [contenteditable='true']"))
  ) {
    return true;
  }
  return (
    document.querySelector(
      "[role='dialog'], [role='menu'], [role='listbox'], [data-radix-popper-content-wrapper]",
    ) !== null
  );
}

const DEFAULT_CREATE: QuickCreateDefaults = {
  ...defaultCreateIds(NATIVE_CAPABILITIES),
  assignee: null,
  containerId: null,
};

/**
 * `/projects/$projectId/planning` — the project's issues as a compact list
 * or a board over the same records, with filters, grouping, quick create
 * and a detail panel. Native planning is the provider: the issues are the
 * project's shared NIP-TD list items, so the To-Do tab, `bee todos` and this
 * page all read and write the same records.
 *
 * Keyboard: C new issue, / search, J/K or ↓/↑ move, Enter open, Esc close.
 */
export function PlanningScreen({
  projectId,
  issueKey,
}: {
  projectId: string;
  /** The `?issue=` the route carries, or null. */
  issueKey: string | null;
}) {
  const { project } = useProjectContainerQuery(projectId);
  const navigate = useNavigate();
  const pulseEnabled = useFeatureEnabled("project-pulse");
  const identity = useIdentityQuery();
  const self = identity.data?.pubkey?.toLowerCase() ?? null;
  const coordinate =
    project && project.id !== LOCAL_GENERAL_ID && project.owner.length > 0
      ? project.address
      : null;
  const provider = resolvePlanningProvider();
  const planning = useNativePlanning(coordinate);
  const capabilities = NATIVE_CAPABILITIES;

  const projectCapabilities = useProjectCapabilities(project);
  const rosterQuery = useProjectRosterQuery(project);
  const todoAccess = todoWriteAccess(
    self,
    project,
    rosterQuery.data ?? project?.members ?? [],
    projectCapabilities,
  );
  const access: PlanningAccess =
    todoAccess.kind === "no-coordinate"
      ? {
          kind: "read-only",
          reason:
            "This project has no published head yet, so it cannot hold issues. Publish it first.",
        }
      : todoAccess;
  const canEdit = access.kind === "writable";

  // People: every person an issue names, resolved to Beekeeper profiles in
  // one batch. Person ids are the provider's; natively they are pubkeys.
  const read = planning.state.kind === "loading" ? null : planning.state.read;
  const viewerId = viewerPersonId(capabilities, read, self);
  const pubkeys = React.useMemo(() => {
    const set = new Set<string>();
    for (const issue of read?.issues ?? []) {
      for (const id of [issue.createdBy, issue.assignee]) {
        const pubkey =
          id === null ? null : personPubkey(capabilities, read, id);
        if (pubkey !== null) set.add(pubkey);
      }
    }
    return [...set];
  }, [read]);
  const profiles = useUsersBatchQuery(pubkeys);
  const profileMap = profiles.data?.profiles;
  const personFor = React.useCallback(
    (personId: string) =>
      todoPerson(
        personPubkey(capabilities, read, personId) ?? personId,
        profileMap,
      ),
    [profileMap, read],
  );

  // Display options are personal and remembered; filters are not.
  const displayKey = planningDisplayKey(self, project?.address ?? projectId);
  const [display, setDisplay] = React.useState<PlanningDisplay>(
    DEFAULT_PLANNING_DISPLAY,
  );
  React.useEffect(() => {
    setDisplay(readPlanningDisplay(displayKey));
  }, [displayKey]);
  const onDisplayChange = React.useCallback(
    (next: PlanningDisplay) => {
      setDisplay(next);
      if (!writePlanningDisplay(displayKey, next)) {
        toast.message("This view could not be remembered on this computer.");
      }
    },
    [displayKey],
  );
  const [filters, setFilters] = React.useState<PlanningFilters>(
    DEFAULT_PLANNING_FILTERS,
  );

  // Selection (keyboard focus) is local; the open issue is in the URL so a
  // reload or a shared link lands on it.
  const [selectedKey, setSelectedKey] = React.useState<string | null>(issueKey);
  const setOpenKey = React.useCallback(
    (key: string | null) => {
      void navigate({
        to: "/projects/$projectId/planning",
        params: { projectId },
        search: key ? { issue: key } : {},
        replace: true,
      });
    },
    [navigate, projectId],
  );

  // In-flight writes, per issue and field, so a row says "Saving…" until
  // the relay has accepted the op.
  const [pending, setPending] = React.useState<Map<string, Set<string>>>(
    () => new Map(),
  );
  const track = React.useCallback(
    async (issue: PlanningIssue, field: string, work: () => Promise<void>) => {
      const mark = (on: boolean) =>
        setPending((current) => {
          const next = new Map(current);
          const fields = new Set(next.get(issue.key) ?? []);
          if (on) fields.add(field);
          else fields.delete(field);
          if (fields.size === 0) next.delete(issue.key);
          else next.set(issue.key, fields);
          return next;
        });
      mark(true);
      try {
        await work();
      } finally {
        mark(false);
      }
    },
    [],
  );
  const run = React.useCallback(
    (issue: PlanningIssue, field: string, work: () => Promise<void>) => {
      track(issue, field, work).catch((error: unknown) => {
        if (error instanceof PlanningPartialWriteError) {
          toast.warning(error.message);
          return;
        }
        toast.error(`Could not save the ${field}: ${message(error)}`);
      });
    },
    [track],
  );
  const isPending = React.useCallback(
    (key: string) => pending.has(key),
    [pending],
  );

  const { mutations } = planning;
  const [createOpen, setCreateOpen] = React.useState(false);
  const [createDefaults, setCreateDefaults] =
    React.useState<QuickCreateDefaults>(DEFAULT_CREATE);
  const openCreate = React.useCallback(
    (defaults: Partial<QuickCreateDefaults> = {}) => {
      setCreateDefaults({
        ...DEFAULT_CREATE,
        containerId: filters.containerId,
        ...defaults,
      });
      setCreateOpen(true);
    },
    [filters.containerId],
  );

  const actions = React.useMemo<PlanningRowActions>(
    () => ({
      open: (issue) => {
        setSelectedKey(issue.key);
        setOpenKey(issue.key);
      },
      select: (issue) => setSelectedKey(issue.key),
      setStatus: (issue, statusId) =>
        run(issue, "status", () => mutations.setStatus(issue, statusId)),
      setPriority: (issue, priorityId) =>
        run(issue, "priority", () => mutations.setPriority(issue, priorityId)),
      setAssignee: (issue, assignee) =>
        run(issue, "assignee", () => mutations.setAssignee(issue, assignee)),
      createIn: (group: PlanningGroup) => {
        switch (display.groupBy) {
          case "status":
            openCreate({ statusId: group.id });
            return;
          case "priority":
            openCreate({ priorityId: group.id });
            return;
          case "assignee":
            openCreate({ assignee: group.assignee ?? null });
            return;
          case "container":
            openCreate({ containerId: group.id });
            return;
          case "none":
            openCreate();
        }
      },
    }),
    [display.groupBy, mutations, openCreate, run, setOpenKey],
  );

  const onCreate = React.useCallback(
    async (input: PlanningCreateInput) => {
      const result = await mutations.createIssue(input);
      setSelectedKey(result.key);
      if (result.failed.length > 0) {
        toast.warning(
          `Created “${input.title}”, but ${result.failed
            .map((f) => `its ${f.field} (${f.message})`)
            .join(
              ", ",
            )} ${result.failed.length === 1 ? "was" : "were"} not confirmed.`,
        );
      } else {
        toast.success(`Created “${input.title}”.`);
      }
    },
    [mutations],
  );

  // Keyboard: the page's own keys, never while typing or inside a menu.
  const searchRef = React.useRef<HTMLInputElement | null>(null);
  const orderRef = React.useRef<readonly string[]>([]);
  const onVisibleOrder = React.useCallback((keys: readonly string[]) => {
    orderRef.current = keys;
  }, []);
  const selectedRef = React.useRef(selectedKey);
  selectedRef.current = selectedKey;
  React.useEffect(() => {
    const focusRow = (key: string) => {
      const target = document.querySelector<HTMLElement>(
        `[data-planning-row-title="${CSS.escape(key)}"]`,
      );
      target?.focus();
      target?.scrollIntoView({ block: "nearest" });
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && issueKey !== null) {
        // A field or a menu handles its own Escape first.
        const target = event.target as HTMLElement | null;
        if (target?.closest("input, textarea")) return;
        if (document.querySelector("[role='dialog'], [role='menu']")) return;
        event.preventDefault();
        setOpenKey(null);
        return;
      }
      if (keyBelongsElsewhere(event)) return;
      switch (event.key) {
        case "c":
          if (!canEdit || read === null) return;
          event.preventDefault();
          openCreate();
          return;
        case "/":
          event.preventDefault();
          searchRef.current?.focus();
          return;
        case "j":
        case "ArrowDown":
        case "k":
        case "ArrowUp": {
          const step = event.key === "j" || event.key === "ArrowDown" ? 1 : -1;
          const next = stepSelection(
            orderRef.current,
            selectedRef.current,
            step,
          );
          if (next === null) return;
          event.preventDefault();
          setSelectedKey(next);
          focusRow(next);
          return;
        }
        case "Enter": {
          const target = event.target as HTMLElement | null;
          if (target?.closest("button, a")) return;
          if (selectedRef.current) {
            event.preventDefault();
            setOpenKey(selectedRef.current);
          }
          return;
        }
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [canEdit, issueKey, openCreate, read, setOpenKey]);

  if (!project) {
    return (
      <div
        className="flex flex-1 items-center justify-center p-4"
        data-testid="planning-missing"
      >
        <p className="text-sm text-muted-foreground">Project not found.</p>
      </div>
    );
  }

  const openIssue =
    issueKey !== null
      ? (read?.issues.find((issue) => issue.key === issueKey) ?? null)
      : null;
  const containerTitle = (id: string) =>
    read?.containers.find((container) => container.id === id)?.title ?? null;

  return (
    <div
      className="flex h-full min-h-0 min-w-0 flex-col p-4"
      data-testid="planning-screen"
    >
      <div>
        <h1 className="break-words text-xl font-semibold text-foreground">
          {project.name}
        </h1>
        <ProjectPageTabs
          active="planning"
          projectId={project.id}
          showPulse={pulseEnabled}
        />
      </div>
      <div className="flex min-h-0 flex-1 gap-0">
        <PlanningView
          access={access}
          actions={actions}
          capabilities={capabilities}
          display={display}
          filters={filters}
          isPending={isPending}
          onCreate={() => openCreate()}
          onDisplayChange={onDisplayChange}
          onFiltersChange={setFilters}
          onVisibleOrder={onVisibleOrder}
          openKey={issueKey}
          personFor={personFor}
          project={project}
          provider={provider}
          searchRef={searchRef}
          selectedKey={selectedKey}
          self={viewerId}
          state={planning.state}
        />
        {issueKey !== null && read !== null ? (
          openIssue ? (
            <PlanningIssueDetail
              actions={{
                close: () => setOpenKey(null),
                setTitle: (issue, title) =>
                  track(issue, "title", () => mutations.setTitle(issue, title)),
                setDescription: (issue, description) =>
                  track(issue, "description", () =>
                    mutations.setDescription(issue, description),
                  ),
                setStatus: actions.setStatus,
                setPriority: actions.setPriority,
                setAssignee: actions.setAssignee,
              }}
              activity={planning.activityFor(openIssue)}
              canEdit={canEdit}
              capabilities={capabilities}
              containerTitle={containerTitle(openIssue.containerId)}
              issue={openIssue}
              key={openIssue.key}
              pendingFields={pending.get(openIssue.key) ?? new Set()}
              personFor={personFor}
              project={project}
              readOnlyReason={
                access.kind === "read-only" ? access.reason : null
              }
            />
          ) : (
            <aside
              className="flex w-[26rem] shrink-0 flex-col gap-2 border-l border-border px-4 py-3"
              data-testid="planning-detail-missing"
            >
              <p className="text-sm text-foreground">
                This issue is not in the current read.
              </p>
              <p className="text-xs text-muted-foreground">
                It was removed, its list was archived, or it belongs to another
                project.
              </p>
              <button
                className="self-start text-xs text-muted-foreground underline hover:text-foreground"
                onClick={() => setOpenKey(null)}
                type="button"
              >
                Close
              </button>
            </aside>
          )
        ) : null}
      </div>
      <PlanningQuickCreate
        capabilities={capabilities}
        containers={read?.containers ?? []}
        defaults={createDefaults}
        onCreate={onCreate}
        onOpenChange={setCreateOpen}
        open={createOpen}
        personFor={personFor}
        project={project}
      />
    </div>
  );
}
