import * as React from "react";
import { Link, useNavigate } from "@tanstack/react-router";
import { Info } from "lucide-react";
import { toast } from "sonner";

import { useCommunities } from "@/features/communities/useCommunities";
import { InstallCrewRolesDialog } from "@/features/agents/ui/InstallCrewRolesDialog";
import { crewRolesInstalledToast } from "@/features/agents/ui/installCrewRolesCopy";
import { ProjectPageTabs } from "@/features/projects-container/ui/ProjectPageTabs";
import { useFeatureEnabled } from "@/shared/features";
import { Skeleton } from "@/shared/ui/skeleton";

import { rolesPageSummary } from "../lib/rolesPageSummary";
import { useProjectPacksView } from "../lib/useProjectPacksView";
import { useProjectTeamSetupSummaryQuery } from "../lib/useProjectTeamSetupSummary";
import type { SeatRow } from "../lib/rolesViewModel";
import {
  rolesUncertaintySentence,
  summarizeRoleReports,
  summarizeRolesUncertainty,
} from "../lib/roleVersionSummary";
import { RoleCard } from "./RoleCard";
import { ProjectTeamSetupWorkbench } from "./ProjectTeamSetupWorkbench";
import { RolePackSnapshots } from "./RolePackSnapshots";
import { RolesHeader } from "./RolesHeader";
import { RolesSummaryStrip } from "./RolesSummaryStrip";
import {
  PROJECT_PACKS_MISSING,
  ROLES_EMPTY,
  ROLES_AGENTS_POINTER,
  ROLES_AGENTS_POINTER_LINK,
  ROLES_LOADING,
  ROLES_UNCERTAINTY_NONE,
  rolesErrorSentence,
} from "./rolesCopy";

function RolesSkeleton() {
  return (
    <output
      aria-label={ROLES_LOADING}
      className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3"
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
 * The project's Roles tab: what each role is for, which agents can take it,
 * which version of its instructions this computer has, and what running
 * agents reported — for the one project the route names.
 *
 * The page answers those four questions in plain sentences and keeps the
 * protocol's own vocabulary (coordinates, provenance labels, the full report
 * history) in a collapsed Technical details section below. The uncertainty
 * line between them is the one place a reader is told, without opening
 * anything, that some of what is above could not be confirmed.
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
    isRefreshing,
    error,
    shelfState,
    packs,
    packsSource,
    rolePackSnapshots,
    packsResolutionIsStale,
    revisionsError,
    provenanceError,
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

  // Shares its query with the setup button, so this adds no second read.
  const { activeCommunity } = useCommunities();
  const setupSummary = useProjectTeamSetupSummaryQuery(
    project?.address ?? "",
    activeCommunity?.relayUrl ?? "",
  );
  // Only mentioned while roles are still built-in defaults (rolesCopy), so a
  // saved draft there is by definition not yet installed from the project.
  const draftInProgress = setupSummary.data != null;

  const { refetchAgents, refetchPacks } = state;
  const onRecheck = React.useCallback(() => {
    refetchPacks();
    refetchAgents();
  }, [refetchAgents, refetchPacks]);

  // Grouped once for the whole grid: every card reads its own role's summary
  // out of this map instead of walking the report list itself.
  const reportSummaries = React.useMemo(
    () => summarizeRoleReports(rolePackSnapshots),
    [rolePackSnapshots],
  );
  // The strip's four counts are set sizes over the same rows the cards draw,
  // so it can never disagree with what is under it.
  const summary = React.useMemo(
    () => rolesPageSummary(view, rolePackSnapshots),
    [rolePackSnapshots, view],
  );
  const uncertainty = React.useMemo(
    () =>
      rolesUncertaintySentence(
        summarizeRolesUncertainty({
          snapshots: rolePackSnapshots,
          resolvedError: error,
          resolvedIsStale: packsResolutionIsStale,
          revisionsError,
          provenanceError,
          reports: executionReports,
          sessions: { kind: shelfState.kind },
        }),
      ),
    [
      error,
      executionReports,
      packsResolutionIsStale,
      provenanceError,
      revisionsError,
      rolePackSnapshots,
      shelfState.kind,
    ],
  );

  // The version the built-in defaults agree on, for the source line. Two
  // built-in versions in one project would make "v<version>" a guess, so the
  // sentence then names none.
  const shippedVersion = React.useMemo(() => {
    const versions = new Set(
      packs
        .filter((pack) => pack.origin === "shipped")
        .map((pack) => pack.version),
    );
    return versions.size === 1 ? ([...versions][0] ?? null) : null;
  }, [packs]);

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

  // Every read this page shows comes from the two refetches the button calls;
  // `useProjectPacksView` exposes a pending flag for the packs read and a
  // loading flag for the reports read, so those two are what "Checking…"
  // reports. No new read and no polling is introduced here.
  const busy = isLoading || isRefreshing || executionReports.isLoading;
  const sourceDetail = {
    checkedAgeSeconds:
      rolePackSnapshots.resolvedAt === null
        ? null
        : Math.max(
            0,
            Math.floor((Date.now() - rolePackSnapshots.resolvedAt) / 1_000),
          ),
    shippedVersion,
    draftInProgress,
  };
  const shelfNotice =
    shelfState.kind !== "ready" && shelfState.kind !== "loading"
      ? {
          kind: shelfState.kind,
          message: shelfState.message,
          detail: shelfState.detail,
        }
      : null;

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
        <RolesHeader
          busy={busy}
          onInstall={() => setInstallOpen(true)}
          onRecheck={onRecheck}
          packCount={packs.length}
          packsSource={packsSource}
          projectName={project.name}
          sourceDetail={sourceDetail}
        />
        <ProjectTeamSetupWorkbench
          projectName={project.name}
          projectRef={project.address}
        />
        {error ? (
          <p
            className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive"
            data-testid="roles-error"
            role="alert"
          >
            {rolesErrorSentence(error)}
          </p>
        ) : null}
        <RolesSummaryStrip summary={summary} />
        <section className="flex flex-col gap-2" data-testid="roles-section">
          {isLoading && view.roles.length === 0 ? (
            <RolesSkeleton />
          ) : view.roles.length === 0 ? (
            <p
              className="text-sm text-muted-foreground"
              data-testid="roles-empty"
            >
              {ROLES_EMPTY}
            </p>
          ) : (
            <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
              {view.roles.map((role) => (
                <RoleCard
                  key={role.role}
                  onOpenSeat={onOpenSeat}
                  reports={reportSummaries.get(role.role) ?? null}
                  role={role}
                />
              ))}
            </div>
          )}
        </section>
        <p
          className="text-xs text-muted-foreground"
          data-testid="roles-agents-pointer"
        >
          {ROLES_AGENTS_POINTER}{" "}
          <Link
            className="underline underline-offset-2"
            params={{ projectId: project.id }}
            to="/projects/$projectId/agents"
          >
            {ROLES_AGENTS_POINTER_LINK}
          </Link>
        </p>
        <p
          className="flex items-center gap-2 text-xs text-muted-foreground"
          data-testid="roles-uncertainty-summary"
          data-uncertain={uncertainty === "" ? "false" : "true"}
        >
          <Info aria-hidden className="size-3.5 shrink-0" />
          <span>
            {uncertainty === "" ? ROLES_UNCERTAINTY_NONE : uncertainty}
          </span>
        </p>
        <RolePackSnapshots
          reports={executionReports}
          resolvedError={error}
          resolvedIsStale={packsResolutionIsStale}
          revisionsError={revisionsError}
          shelfNotice={shelfNotice}
          snapshots={rolePackSnapshots}
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
