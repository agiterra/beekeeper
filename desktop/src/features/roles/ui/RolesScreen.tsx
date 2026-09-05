import { Skeleton } from "@/shared/ui/skeleton";

import type { SeatRow } from "../lib/rolesViewModel";
import type { RolesViewState } from "../lib/useRolesView";
import { AgentsByProject } from "./AgentsByProject";
import { RoleCard } from "./RoleCard";
import {
  packsSourceSentence,
  ROLES_EMPTY,
  ROLES_LOADING,
  ROLES_NO_PROJECT,
  ROLES_PROJECT_PICKER_ARIA,
  ROLES_PROJECT_PICKER_LABEL,
  ROLES_SECTION_TITLE,
  ROLES_SUBTITLE,
  ROLES_TITLE,
  rolesErrorSentence,
} from "./rolesCopy";

function RolesSkeleton() {
  return (
    <output
      aria-label={ROLES_LOADING}
      className="grid gap-2 md:grid-cols-2 xl:grid-cols-3"
      data-testid="roles-loading"
    >
      {["a", "b", "c"].map((key) => (
        <div
          className="flex flex-col gap-2 rounded-lg border border-border p-3"
          key={key}
        >
          <Skeleton className="h-4 w-1/3" />
          <Skeleton className="h-3 w-2/3" />
          <Skeleton className="h-3 w-full" />
          <Skeleton className="h-3 w-5/6" />
        </div>
      ))}
    </output>
  );
}

function RolesHeaderRow({ state }: { state: RolesViewState }) {
  const { project, projects, chooseProject, packs, packsSource } = state;
  const sentence = project
    ? packsSourceSentence(project.name, packs.length, packsSource)
    : ROLES_NO_PROJECT;
  return (
    <div
      className="flex flex-wrap items-center gap-3"
      data-project-source={state.source}
      data-testid="roles-header"
    >
      <label className="flex items-center gap-1 text-xs text-muted-foreground">
        <span>{ROLES_PROJECT_PICKER_LABEL}</span>
        <select
          aria-label={ROLES_PROJECT_PICKER_ARIA}
          className="h-8 rounded-lg border border-border bg-background px-2 text-sm text-foreground outline-hidden focus-visible:ring-1 focus-visible:ring-ring"
          data-testid="roles-project-picker"
          disabled={projects.length === 0}
          onChange={(event) => chooseProject(event.target.value)}
          value={project?.id ?? ""}
        >
          {projects.map((candidate) => (
            <option key={candidate.id} value={candidate.id}>
              {candidate.name}
            </option>
          ))}
        </select>
      </label>
      <p
        className="min-w-0 truncate text-xs text-muted-foreground"
        data-testid="roles-source-sentence"
        title={sentence}
      >
        {sentence}
      </p>
    </div>
  );
}

/**
 * The Roles tab body: header (project picker + source sentence), the role
 * cards, and agents by project.
 *
 * The packs read can be loading or failed while the shelf and the agents
 * already answered, so Section A shows its skeleton or the backend's error
 * sentence *and* still lists the rows the seats and agents alone produce —
 * a role a seat names is a fact whether or not the ladder was readable.
 */
export function RolesScreen({
  state,
  onOpenSeat,
}: {
  state: RolesViewState;
  onOpenSeat?: (seat: SeatRow) => void;
}) {
  const { view, isLoading, error, shelfState } = state;
  return (
    <div
      className="flex h-full min-h-0 flex-col gap-4 overflow-y-auto p-4"
      data-testid="roles-screen"
    >
      <header className="flex flex-col gap-1">
        <h1 className="text-base font-medium text-foreground">{ROLES_TITLE}</h1>
        <p
          className="text-2xs text-muted-foreground"
          data-testid="roles-subtitle"
        >
          {ROLES_SUBTITLE}
        </p>
      </header>
      <RolesHeaderRow state={state} />
      {error ? (
        <p
          className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive"
          data-testid="roles-error"
          role="alert"
        >
          {rolesErrorSentence(error)}
        </p>
      ) : null}
      {shelfState.kind !== "ready" && shelfState.kind !== "loading" ? (
        <p
          className="text-xs text-muted-foreground"
          data-shelf-state={shelfState.kind}
          data-testid="roles-shelf-notice"
        >
          {shelfState.message}
          {shelfState.detail ? ` — ${shelfState.detail}` : null}
        </p>
      ) : null}
      <section className="flex flex-col gap-2" data-testid="roles-section">
        <h2 className="text-sm font-medium text-foreground">
          {ROLES_SECTION_TITLE}
        </h2>
        {isLoading && view.roles.length === 0 ? (
          <RolesSkeleton />
        ) : view.roles.length === 0 ? (
          <p
            className="text-xs text-muted-foreground"
            data-testid="roles-empty"
          >
            {ROLES_EMPTY}
          </p>
        ) : (
          <div className="grid gap-2 md:grid-cols-2 xl:grid-cols-3">
            {view.roles.map((role) => (
              <RoleCard key={role.role} onOpenSeat={onOpenSeat} role={role} />
            ))}
          </div>
        )}
      </section>
      <AgentsByProject
        byProject={view.byProject}
        onOpenSeat={onOpenSeat}
        unplaced={view.unplaced}
      />
    </div>
  );
}
