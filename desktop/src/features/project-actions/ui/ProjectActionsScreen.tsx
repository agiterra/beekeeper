import { ProjectPageTabs } from "@/features/projects-container/ui/ProjectPageTabs";
import { useFeatureEnabled } from "@/shared/features";
import { Skeleton } from "@/shared/ui/skeleton";

import { useProjectActions } from "../lib/useProjectActions";
import { ProjectActionCard } from "./ProjectActionCard";

export const PROJECT_ACTIONS_MISSING =
  "This project is not readable from here.";

/**
 * `/projects/$projectId/actions` — the project's `beekeeper/actions.yml`
 * entries as the relay holds them (kind:30620 definitions that name this
 * project), each with its latest runs and what their records prove.
 */
export function ProjectActionsScreen({ projectId }: { projectId: string }) {
  const { project, actions, isLoading, error, refresh } =
    useProjectActions(projectId);
  const pulseEnabled = useFeatureEnabled("project-pulse");

  if (!project) {
    return (
      <div
        className="flex flex-1 items-center justify-center p-4"
        data-testid="project-actions-missing"
      >
        <p className="text-sm text-muted-foreground">
          {PROJECT_ACTIONS_MISSING}
        </p>
      </div>
    );
  }

  return (
    <div
      className="flex h-full min-h-0 min-w-0 flex-col overflow-y-auto p-4"
      data-testid="project-actions-screen"
    >
      <div>
        <h1 className="break-words text-xl font-semibold text-foreground">
          {project.name}
        </h1>
        <ProjectPageTabs
          active="actions"
          projectId={project.id}
          showPulse={pulseEnabled}
        />
      </div>
      {error ? (
        <p className="mb-3 text-sm text-destructive" role="alert">
          Could not read this project's actions: {error}
        </p>
      ) : null}
      {isLoading ? (
        <div className="space-y-3" data-testid="project-actions-loading">
          <Skeleton className="h-24 w-full rounded-xl" />
          <Skeleton className="h-24 w-full rounded-xl" />
        </div>
      ) : actions.length === 0 ? (
        <div
          className="rounded-xl border border-dashed border-border/70 p-6 text-sm text-muted-foreground"
          data-testid="project-actions-empty"
        >
          <p>No actions are published for this project.</p>
          <p className="mt-2">
            Publish the repository's{" "}
            <code className="font-mono text-xs">beekeeper/actions.yml</code>{" "}
            with{" "}
            <code className="break-all font-mono text-xs">
              bee actions publish --project {project.address} --channel
              &lt;channel uuid&gt;
            </code>
            .
          </p>
        </div>
      ) : (
        <div className="space-y-4">
          {actions.map((action) => (
            <ProjectActionCard
              action={action}
              key={action.workflow.id}
              onChanged={refresh}
            />
          ))}
        </div>
      )}
    </div>
  );
}
