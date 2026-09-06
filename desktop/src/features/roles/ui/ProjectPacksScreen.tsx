import * as React from "react";
import { useNavigate } from "@tanstack/react-router";
import { toast } from "sonner";

import { InstallCrewRolesDialog } from "@/features/agents/ui/InstallCrewRolesDialog";
import { crewRolesInstalledToast } from "@/features/agents/ui/installCrewRolesCopy";
import { ProjectPageTabs } from "@/features/projects-container/ui/ProjectPageTabs";
import { useFeatureEnabled } from "@/shared/features";
import { Button } from "@/shared/ui/button";
import { Skeleton } from "@/shared/ui/skeleton";

import { useProjectPacksView } from "../lib/useProjectPacksView";
import type { SeatRow } from "../lib/rolesViewModel";
import { AgentsByProject } from "./AgentsByProject";
import { RoleCard } from "./RoleCard";
import { RolePackSnapshots } from "./RolePackSnapshots";
import {
  INSTALL_ROLES_BUTTON_LABEL,
  packsSourceSentence,
  PROJECT_PACKS_MISSING,
  ROLES_EMPTY,
  ROLES_LOADING,
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

/**
 * The project's Packs tab: what each role has, who carries it, and who is
 * seated in it — for the one project the route names. `RolesScreen` minus
 * the project picker (the route supplies the project now), plus the
 * Install-roles button that used to live on the Agents tab's picker toolbar
 * (§E "moved, not cut" — pack installation is project-scoped, and this route
 * finally names the project instead of resolving one nobody can see).
 */
export function ProjectPacksScreen({ projectId }: { projectId: string }) {
  const state = useProjectPacksView(projectId);
  const pulseEnabled = useFeatureEnabled("project-pulse");
  const navigate = useNavigate();
  const [installOpen, setInstallOpen] = React.useState(false);

  const {
    project,
    view,
    isLoading,
    error,
    shelfState,
    packs,
    packsSource,
    rolePackSnapshots,
    packsResolutionIsStale,
    executionReports,
  } = state;

  const onOpenSeat = React.useCallback(
    (seat: SeatRow) => {
      void navigate({
        to: "/coding-sessions/$channelId/$generationId",
        params: { channelId: seat.channelId, generationId: seat.generationId },
        search: { surface: "main" },
      });
    },
    [navigate],
  );

  if (!project) {
    return (
      <div
        className="flex flex-1 items-center justify-center p-4"
        data-testid="project-packs-missing"
      >
        <p className="text-sm text-muted-foreground">{PROJECT_PACKS_MISSING}</p>
      </div>
    );
  }

  const sentence = packsSourceSentence(project.name, packs.length, packsSource);

  return (
    <div
      className="flex h-full min-h-0 flex-col overflow-y-auto p-4"
      data-testid="project-packs-screen"
    >
      <div>
        <h1 className="text-xl font-semibold text-foreground">
          {project.name}
        </h1>
        <ProjectPageTabs
          active="packs"
          projectId={project.id}
          showPulse={pulseEnabled}
        />
      </div>
      <div className="flex flex-col gap-4">
        <header className="flex flex-col gap-1">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <h2 className="text-base font-medium text-foreground">
              {ROLES_TITLE}
            </h2>
            <Button
              data-testid="project-packs-install"
              onClick={() => setInstallOpen(true)}
              type="button"
              variant="outline"
            >
              {INSTALL_ROLES_BUTTON_LABEL}
            </Button>
          </div>
          <p
            className="text-2xs text-muted-foreground"
            data-testid="roles-subtitle"
          >
            {ROLES_SUBTITLE}
          </p>
          <p
            className="min-w-0 truncate text-xs text-muted-foreground"
            data-testid="packs-source-sentence"
            title={sentence.shaTitle ?? sentence.text}
          >
            {sentence.text}
          </p>
        </header>
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
        <RolePackSnapshots
          reports={executionReports}
          resolvedError={error}
          resolvedIsStale={packsResolutionIsStale}
          snapshots={rolePackSnapshots}
        />
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

      <InstallCrewRolesDialog
        onInstalled={(result) => {
          state.refetchPacks();
          state.refetchAgents();
          toast.success(crewRolesInstalledToast(result));
        }}
        onOpenChange={setInstallOpen}
        open={installOpen}
        project={{ address: project.address, name: project.name }}
      />
    </div>
  );
}
