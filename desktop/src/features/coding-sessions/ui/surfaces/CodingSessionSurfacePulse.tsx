import * as React from "react";
import { useNavigate } from "@tanstack/react-router";
import { Activity, ArrowUpRight } from "lucide-react";

import {
  useProjectPulseDigest,
  usePulseMissionRows,
} from "@/features/project-pulse/lib/pulseQueries";
import {
  useProjectPulseChannelIds,
  useProjectPulseProject,
} from "@/features/project-pulse/ui/ProjectPulseScreen";
import {
  ProjectPulseView,
  type ProjectPulseViewState,
} from "@/features/project-pulse/ui/ProjectPulseView";
import { usePulseAuthorNames } from "@/features/project-pulse/ui/usePulseAuthorNames";
import { LOCAL_GENERAL_ID } from "@/features/projects-container/lib/projectContainerModel";

import { CodingSessionSurfaceSubheader } from "../CodingSessionChangesRailSubheader";
import type {
  CodingSessionSurfaceAvailability,
  CodingSessionSurfaceCtx,
} from "./codingSessionSurfaceContext";
import type { CodingSessionSurfaceDefinition } from "./codingSessionSurfaceRegistry";
import { CodingSessionSurfacePlaceholder } from "./CodingSessionSurfaceDevicePlaceholder";

/** Pulse: when it can open, or the sentence why not (§3, SV-23). */
export function codingSessionSurfacePulseAvailability(
  ctx: CodingSessionSurfaceCtx,
): CodingSessionSurfaceAvailability {
  return ctx.projectRef !== null
    ? { available: true }
    : { available: false, reason: "This session belongs to no project." };
}

/** A project coordinate's `d` (`30621:<owner>:<d>`), or the ref itself. */
function projectIdFromRef(projectRef: string): string {
  const parts = projectRef.split(":");
  return parts.length >= 3 ? parts.slice(2).join(":") : projectRef;
}

/**
 * The owning project's Pulse, read exactly as the project's Pulse tab reads
 * it (`ProjectPulseScreen`), with this session's card first. Declared work
 * and this computer's worktrees are left to the full Pulse, one click away,
 * and the panel says so rather than letting their absence read as none.
 */
function CodingSessionPulseBody({ ctx }: { ctx: CodingSessionSurfaceCtx }) {
  const projectId =
    ctx.project?.id ??
    (ctx.projectRef === null ? "" : projectIdFromRef(ctx.projectRef));
  const { project, isLoading: projectLoading } =
    useProjectPulseProject(projectId);
  const { channelIds, unresolved } = useProjectPulseChannelIds(project);
  const isFallback = project?.id === LOCAL_GENERAL_ID;
  const coordinate = project && !isFallback ? project.address : null;
  const pulse = useProjectPulseDigest(coordinate, channelIds, unresolved);
  const state: ProjectPulseViewState = React.useMemo(() => {
    if (isFallback) return { kind: "unavailable" };
    if (!project) {
      return projectLoading ? { kind: "loading" } : { kind: "unavailable" };
    }
    if (pulse.kind === "loading") {
      return pulse.digest
        ? { kind: "ready", digest: pulse.digest, refreshing: true }
        : { kind: "loading" };
    }
    return pulse;
  }, [isFallback, project, projectLoading, pulse]);
  const authorNames = usePulseAuthorNames(
    state.kind === "ready" || state.kind === "partial" ? state.digest : null,
  );
  const missionNames = React.useMemo(
    () => Object.fromEntries(authorNames),
    [authorNames],
  );
  const missions = usePulseMissionRows(coordinate, channelIds, {
    digest: pulse.digest,
    displayNames: missionNames,
  });
  // One clock for the panel, ticking once a minute, as the Pulse tab's does.
  const [nowSeconds, setNowSeconds] = React.useState(() =>
    Math.floor(Date.now() / 1_000),
  );
  React.useEffect(() => {
    const timer = window.setInterval(
      () => setNowSeconds(Math.floor(Date.now() / 1_000)),
      60_000,
    );
    return () => window.clearInterval(timer);
  }, []);
  const navigate = useNavigate();
  const openFullPulse =
    project && !isFallback
      ? () =>
          void navigate({
            to: "/projects/$projectId",
            params: { projectId: project.id },
            search: { tab: "pulse" },
          })
      : null;
  return (
    <>
      <CodingSessionSurfaceSubheader
        actions={
          openFullPulse ? (
            <button
              className="inline-flex items-center gap-1 rounded-md px-1.5 py-1 text-2xs font-medium text-primary hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              data-testid="coding-session-pulse-open-full"
              onClick={openFullPulse}
              type="button"
            >
              Open full Pulse
              <ArrowUpRight aria-hidden className="size-3" />
            </button>
          ) : null
        }
        meta={project ? project.name : null}
        title="Pulse"
      />
      <div
        className="min-h-0 flex-1 overflow-y-auto overscroll-contain"
        data-testid="coding-session-pulse-surface"
      >
        <ProjectPulseView
          authorNames={authorNames}
          leadSessionKey={ctx.umbrella.sessionRef ?? ctx.sessionKey}
          missions={missions}
          nowSeconds={nowSeconds}
          state={state}
        />
        <p className="px-4 pb-4 text-2xs text-muted-foreground">
          Declared work and this computer's worktrees are on the full Pulse.
        </p>
      </div>
    </>
  );
}

/** The Pulse surface: the owning project's Pulse, this session first. */
export function CodingSessionSurfacePulsePanel({
  ctx,
}: {
  ctx: CodingSessionSurfaceCtx;
}) {
  const availability = codingSessionSurfacePulseAvailability(ctx);
  if (!availability.available) {
    return (
      <CodingSessionSurfacePlaceholder
        icon={Activity}
        id="pulse"
        label="Pulse"
        reason={availability.reason}
      />
    );
  }
  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      data-available="true"
      data-testid="coding-session-surface-panel-pulse"
    >
      <CodingSessionPulseBody ctx={ctx} />
    </div>
  );
}

export const codingSessionSurfacePulse: CodingSessionSurfaceDefinition = {
  id: "pulse",
  label: "Pulse",
  icon: Activity,
  shortcut: "U",
  order: 80,
  placement: "right",
  lenses: ["conversation", "mission"],
  availability: codingSessionSurfacePulseAvailability,
  Panel: CodingSessionSurfacePulsePanel,
};
