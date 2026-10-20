import { ListTodo } from "lucide-react";
import * as React from "react";

import type { TodoPerson } from "@/features/project-todos/lib/todoPeople";
import type { ProjectContainer } from "@/features/projects-container/lib/projectContainerModel";
import { Button } from "@/shared/ui/button";

import type {
  PlanningAccess,
  PlanningCapabilities,
  PlanningIssue,
  PlanningProviderDescriptor,
  PlanningProviderId,
  PlanningState,
} from "../lib/planningTypes";
import {
  type PlanningDisplay,
  type PlanningFilters,
  hasActiveFilters,
  planningView,
} from "../lib/planningView";
import {
  PlanningIssueList,
  type PlanningRowActions,
} from "./PlanningIssueList";
import { PlanningProviderControl } from "./PlanningProviderControl";
import { PlanningToolbar } from "./PlanningToolbar";

const PlanningBoard = React.lazy(async () => {
  const module = await import("./PlanningBoard");
  return { default: module.PlanningBoard };
});

export type PlanningViewProps = {
  project: ProjectContainer;
  /** Every provider the control lists, available or not. */
  providers: readonly PlanningProviderDescriptor[];
  /** The provider this view reads, if any. */
  activeProvider: PlanningProviderId | null;
  onChooseProvider?: (id: PlanningProviderId) => void;
  state: PlanningState;
  access: PlanningAccess;
  capabilities: PlanningCapabilities;
  display: PlanningDisplay;
  filters: PlanningFilters;
  self: string | null;
  selectedKey: string | null;
  openKey: string | null;
  isPending: (key: string) => boolean;
  personFor: (pubkey: string) => TodoPerson;
  actions: PlanningRowActions;
  searchRef: React.RefObject<HTMLInputElement | null>;
  onDisplayChange: (display: PlanningDisplay) => void;
  onFiltersChange: (filters: PlanningFilters) => void;
  onCreate: () => void;
  /** Receives the keyboard order of the issues currently shown. */
  onVisibleOrder?: (keys: readonly string[]) => void;
};

type Notice = { key: string; text: string; tone: "info" | "warn" };

/**
 * The Planning body: notices, the toolbar, and the issues as a list or a
 * board. Presentational: the screen owns the read, the writes, the
 * selection and the display options.
 */
export function PlanningView({
  project,
  providers,
  activeProvider,
  onChooseProvider,
  state,
  access,
  capabilities,
  display,
  filters,
  self,
  selectedKey,
  openKey,
  isPending,
  personFor,
  actions,
  searchRef,
  onDisplayChange,
  onFiltersChange,
  onCreate,
  onVisibleOrder,
}: PlanningViewProps) {
  const read =
    state.kind === "loading" || state.kind === "unavailable"
      ? null
      : state.read;
  const issues = read?.issues ?? [];
  const containers = read?.containers ?? [];
  const nameOf = React.useCallback(
    (pubkey: string) => personFor(pubkey).name,
    [personFor],
  );
  // A board always has columns: "no grouping" boards by status.
  const effective = React.useMemo<PlanningDisplay>(
    () =>
      display.layout === "board" && display.groupBy === "none"
        ? { ...display, groupBy: "status" }
        : display,
    [display],
  );
  const view = React.useMemo(
    () =>
      planningView(
        issues,
        filters,
        effective,
        capabilities,
        containers,
        self,
        nameOf,
      ),
    [capabilities, containers, effective, filters, issues, nameOf, self],
  );
  const order = React.useMemo(
    () => view.groups.flatMap((group) => group.issues.map((i) => i.key)),
    [view.groups],
  );
  React.useEffect(() => {
    onVisibleOrder?.(order);
  }, [onVisibleOrder, order]);
  const byKey = React.useMemo(
    () => new Map(issues.map((issue) => [issue.key, issue])),
    [issues],
  );
  const issueByKey = React.useCallback(
    (key: string): PlanningIssue | undefined => byKey.get(key),
    [byKey],
  );
  const canEdit = access.kind === "writable";

  const notices: Notice[] = [];
  if (access.kind === "loading" && read) {
    notices.push({
      key: "access-loading",
      text: "Checking whether you can edit this project's issues…",
      tone: "info",
    });
  }
  // An unavailable state says why in the body; the same reason again as a
  // read-only notice would only repeat it.
  if (access.kind === "read-only" && state.kind !== "unavailable") {
    notices.push({
      key: "read-only",
      text: `Read-only: ${access.reason}`,
      tone: "info",
    });
  }
  if (state.kind === "error") {
    notices.push({
      key: "error",
      text: read
        ? `Could not refresh the issues: ${state.message}. Showing the last read, which may be out of date.`
        : `Could not read the issues: ${state.message}`,
      tone: "warn",
    });
  }
  if (read?.truncated) {
    notices.push({
      key: "truncated",
      text: "This read stopped early: the relay returned more history than was fetched, so older issues may be missing.",
      tone: "warn",
    });
  }
  if (read && read.unreadable > 0) {
    notices.push({
      key: "unreadable",
      text: `${read.unreadable} change${read.unreadable === 1 ? "" : "s"} could not be applied (malformed, or naming a list or issue that does not exist).`,
      tone: "warn",
    });
  }

  let body: React.ReactNode;
  if (state.kind === "unavailable") {
    body = (
      <div
        className="flex flex-col items-center gap-1 py-12 text-center"
        data-testid="planning-unavailable"
      >
        <p className="text-sm text-foreground">
          Planning is not available here.
        </p>
        <p className="max-w-sm text-xs text-muted-foreground">{state.reason}</p>
      </div>
    );
  } else if (!read) {
    body =
      state.kind === "error" ? null : (
        <p
          className="px-1 py-6 text-sm text-muted-foreground"
          data-testid="planning-loading"
        >
          Reading issues…
        </p>
      );
  } else if (issues.length === 0) {
    body = (
      <div
        className="flex flex-col items-center gap-2 py-12 text-center"
        data-testid="planning-empty"
      >
        <ListTodo
          aria-hidden="true"
          className="h-6 w-6 text-muted-foreground"
        />
        <p className="text-sm text-foreground">No issues yet.</p>
        <p className="max-w-sm text-xs text-muted-foreground">
          Issues are the items of this project's shared lists. Every member and
          agent sees the same ones.
        </p>
        {canEdit ? (
          <Button
            data-testid="planning-empty-create"
            onClick={onCreate}
            size="sm"
          >
            Create the first issue
          </Button>
        ) : null}
      </div>
    );
  } else if (view.matched === 0) {
    body = (
      <div
        className="flex flex-col items-center gap-1 py-12 text-center"
        data-testid="planning-no-matches"
      >
        <p className="text-sm text-foreground">No issues match this view.</p>
        <p className="text-xs text-muted-foreground">
          {hasActiveFilters(filters)
            ? "Clear the filters to see more."
            : `${issues.length} issue${issues.length === 1 ? "" : "s"} in other views.`}
        </p>
      </div>
    );
  } else if (display.layout === "board") {
    body = (
      <React.Suspense
        fallback={
          <p className="px-1 py-6 text-sm text-muted-foreground">
            Opening board…
          </p>
        }
      >
        <PlanningBoard
          actions={actions}
          canEdit={canEdit}
          capabilities={capabilities}
          groupBy={effective.groupBy}
          groups={view.groups}
          isPending={isPending}
          issueByKey={issueByKey}
          personFor={personFor}
          selectedKey={selectedKey}
        />
      </React.Suspense>
    );
  } else {
    body = (
      <PlanningIssueList
        actions={actions}
        canEdit={canEdit}
        capabilities={capabilities}
        containers={containers}
        groups={view.groups}
        isPending={isPending}
        openKey={openKey}
        personFor={personFor}
        project={project}
        selectedKey={selectedKey}
        showContainer={containers.length > 1 && display.groupBy !== "container"}
      />
    );
  }

  return (
    <div
      className="flex min-h-0 min-w-0 flex-1 flex-col gap-3"
      data-access={access.kind}
      data-testid="planning-view"
    >
      <div className="flex items-center gap-2">
        <p
          className="text-xs text-muted-foreground"
          data-testid="planning-count"
        >
          {read
            ? `${view.matched} of ${issues.length} issue${issues.length === 1 ? "" : "s"}`
            : " "}
          {state.kind === "ready" && state.refreshing ? " · refreshing" : ""}
        </p>
        <span className="flex-1" />
        <PlanningProviderControl
          active={activeProvider}
          onChoose={onChooseProvider}
          providers={providers}
        />
      </div>
      {notices.length > 0 ? (
        <ul className="flex flex-col gap-1" data-testid="planning-notices">
          {notices.map((notice) => (
            <li
              className={
                notice.tone === "warn"
                  ? "rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs text-foreground"
                  : "rounded-md border border-border bg-muted/40 px-3 py-2 text-xs text-muted-foreground"
              }
              data-testid={`planning-notice-${notice.key}`}
              key={notice.key}
            >
              {notice.text}
            </li>
          ))}
        </ul>
      ) : null}
      {read ? (
        <PlanningToolbar
          canCreate={canEdit}
          capabilities={capabilities}
          containers={containers}
          display={display}
          filters={filters}
          onCreate={onCreate}
          onDisplayChange={onDisplayChange}
          onFiltersChange={onFiltersChange}
          searchRef={searchRef}
        />
      ) : null}
      <div className="min-h-0 flex-1 overflow-y-auto">{body}</div>
    </div>
  );
}
