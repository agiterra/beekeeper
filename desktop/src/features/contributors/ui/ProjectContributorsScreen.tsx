import { ProjectPageTabs } from "@/features/projects-container/ui/ProjectPageTabs";
import { useFeatureEnabled } from "@/shared/features";
import { Skeleton } from "@/shared/ui/skeleton";

import { useProjectContributors } from "../lib/useProjectContributors";
import {
  CONTRIBUTORS_EMPTY,
  CONTRIBUTORS_FOOTNOTE,
  CONTRIBUTORS_LANDINGS_NOTE,
  CONTRIBUTORS_LOADING,
  CONTRIBUTORS_PROJECT_MISSING,
  CONTRIBUTORS_SUBTITLE,
  CONTRIBUTORS_TITLE,
  contributorLastSeenText,
  contributorNameText,
  contributorRolesText,
  contributorSeatsText,
  contributorStateAttr,
  contributorsNoticeSentence,
} from "./contributorsCopy";

function ContributorsSkeleton() {
  return (
    <output
      aria-label={CONTRIBUTORS_LOADING}
      className="flex flex-col gap-2"
      data-testid="contributors-loading"
    >
      {["a", "b", "c"].map((key) => (
        <div
          className="flex items-center gap-3 rounded-lg border border-border p-3"
          key={key}
        >
          <Skeleton className="h-4 w-1/4" />
          <Skeleton className="h-3 w-1/5" />
          <Skeleton className="h-3 w-1/6" />
          <Skeleton className="h-3 w-1/4" />
        </div>
      ))}
    </output>
  );
}

/**
 * The project's Contributors tab (§C) — seat history for the one project the
 * route names: every agent that holds or held a seat here, with only the two
 * membership states this computer has evidence for. Never "available",
 * "eligible", or "offered" — those are consent states no wire record proves
 * (ruling §1).
 */
export function ProjectContributorsScreen({
  projectId,
}: {
  projectId: string;
}) {
  const { project, rows, isLoading, shelfState } =
    useProjectContributors(projectId);
  const pulseEnabled = useFeatureEnabled("project-pulse");

  if (!project) {
    return (
      <div
        className="flex flex-1 items-center justify-center p-4"
        data-testid="project-contributors-missing"
      >
        <p className="text-sm text-muted-foreground">
          {CONTRIBUTORS_PROJECT_MISSING}
        </p>
      </div>
    );
  }

  return (
    <div
      className="flex h-full min-h-0 flex-col overflow-y-auto p-4"
      data-testid="project-contributors-screen"
    >
      <div>
        <h1 className="text-xl font-semibold text-foreground">
          {project.name}
        </h1>
        <ProjectPageTabs
          active="contributors"
          projectId={project.id}
          showPulse={pulseEnabled}
        />
      </div>
      <div className="flex flex-col gap-4">
        <header className="flex flex-col gap-1">
          <h2 className="text-base font-medium text-foreground">
            {CONTRIBUTORS_TITLE}
          </h2>
          <p
            className="text-2xs text-muted-foreground"
            data-testid="contributors-subtitle"
          >
            {CONTRIBUTORS_SUBTITLE}
          </p>
        </header>

        {shelfState.kind !== "ready" && shelfState.kind !== "loading" ? (
          <p
            className="text-xs text-muted-foreground"
            data-shelf-state={shelfState.kind}
            data-testid="contributors-notice"
          >
            {contributorsNoticeSentence(shelfState.message, shelfState.detail)}
          </p>
        ) : null}

        {isLoading && rows.length === 0 ? (
          <ContributorsSkeleton />
        ) : rows.length === 0 ? (
          <p
            className="text-xs text-muted-foreground"
            data-testid="contributors-empty"
          >
            {CONTRIBUTORS_EMPTY}
          </p>
        ) : (
          <table
            className="w-full border-collapse text-sm"
            data-testid="contributors-table"
          >
            <thead>
              <tr className="border-b border-border text-left text-2xs uppercase text-muted-foreground">
                <th className="py-1 pr-2 font-medium">Name</th>
                <th className="py-1 pr-2 font-medium">Role(s)</th>
                <th className="py-1 pr-2 font-medium">Seats</th>
                <th className="py-1 pr-2 font-medium">Last seen</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr
                  className="border-b border-border/60"
                  data-contributor-state={contributorStateAttr(row)}
                  key={row.agentPubkey}
                >
                  <td
                    className="py-1.5 pr-2 font-medium text-foreground"
                    data-testid="contributor-name"
                    title={row.agentPubkey}
                  >
                    {contributorNameText(row)}
                  </td>
                  <td
                    className="py-1.5 pr-2 text-muted-foreground"
                    data-testid="contributor-roles"
                  >
                    {contributorRolesText(row.roles)}
                  </td>
                  <td
                    className="py-1.5 pr-2 text-muted-foreground"
                    data-testid="contributor-seats"
                  >
                    {contributorSeatsText(row.seatCount)}
                  </td>
                  <td
                    className="py-1.5 pr-2 text-muted-foreground"
                    data-testid="contributor-last-seen"
                  >
                    {contributorLastSeenText(row)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}

        <p
          className="text-2xs text-muted-foreground"
          data-testid="contributors-landings-note"
        >
          {CONTRIBUTORS_LANDINGS_NOTE}
        </p>
        <p
          className="text-2xs text-muted-foreground"
          data-testid="contributors-footnote"
        >
          {CONTRIBUTORS_FOOTNOTE}
        </p>
      </div>
    </div>
  );
}
