import { ProjectPageTabs } from "@/features/projects-container/ui/ProjectPageTabs";
import { useIdentityQuery } from "@/shared/api/hooks";
import { Button } from "@/shared/ui/button";
import { useFeatureEnabled } from "@/shared/features";
import { Skeleton } from "@/shared/ui/skeleton";

import { useProjectActions } from "../lib/useProjectActions";
import { useProjectCodeRefTip } from "../lib/useProjectCodeRefTip";
import { useProjectAgentsRepo } from "../lib/useProjectAgentsRepo";
import { ProjectActionCard } from "./ProjectActionCard";

export const PROJECT_ACTIONS_MISSING =
  "This project is not readable from here.";

/**
 * `/projects/$projectId/actions` — the project's `actions.yml` entries (the
 * agents repository's root, spec § 4.11) as the relay holds them (kind:30620
 * definitions that name this project), each with its latest runs and what
 * their records prove.
 *
 * **Opening it changes nothing.** It reads where this computer keeps the
 * project's agents repository; establishing that clone — which synchronizes
 * a managed cache with a forced checkout and clean, and writes this
 * computer's execution configuration — is the Prepare control, offered to
 * the project's owner (Astra's Wave 2 review, finding 11).
 */
export function ProjectActionsScreen({ projectId }: { projectId: string }) {
  const { project, actions, isLoading, error, readability, refresh } =
    useProjectActions(projectId);
  const pulseEnabled = useFeatureEnabled("project-pulse");
  const agentsRepo = useProjectAgentsRepo(project?.address ?? null);
  const identity = useIdentityQuery();
  const tipQuery = useProjectCodeRefTip(
    project?.address ?? null,
    project?.repoAddrs ?? [],
  );
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
      <p
        className="mb-3 flex flex-wrap items-center gap-2 text-xs text-muted-foreground"
        data-testid="project-actions-agents-repo-note"
      >
        <span>{agentsRepo.prepareNote ?? agentsRepo.note}</span>
        {agentsRepo.prepareError ? (
          <span className="text-destructive">{agentsRepo.prepareError}</span>
        ) : null}
        {agentsRepo.status?.recorded === false &&
        identity.data?.pubkey?.toLowerCase() === project.owner.toLowerCase() ? (
          <Button
            data-testid="project-actions-prepare-agents-repo"
            disabled={agentsRepo.preparing}
            onClick={() => agentsRepo.prepare()}
            size="sm"
            type="button"
            variant="outline"
          >
            {agentsRepo.preparing ? "Preparing…" : "Prepare"}
          </Button>
        ) : null}
      </p>
      {isLoading ? (
        <div className="space-y-3" data-testid="project-actions-loading">
          <Skeleton className="h-24 w-full rounded-xl" />
          <Skeleton className="h-24 w-full rounded-xl" />
        </div>
      ) : readability.kind !== "readable" ? (
        <div
          className="rounded-xl border border-dashed border-amber-500/60 p-6 text-sm text-muted-foreground"
          data-testid="project-actions-unread"
          role="status"
        >
          <p>
            {readability.kind === "channels-unreadable"
              ? `This project's actions were not read: the channel list could not be read (${readability.error}).`
              : "This project's actions were not read: no channel of this project is readable from here, so no query was sent. This is not the same as having no actions."}
          </p>
          <p className="mt-2">
            An action is published into the channel{" "}
            <code className="font-mono text-xs">
              bee actions publish --channel
            </code>{" "}
            named — including a team session&apos;s transport channel. Join or
            reopen that channel and this tab will ask again.
          </p>
        </div>
      ) : actions.length === 0 ? (
        <div
          className="rounded-xl border border-dashed border-border/70 p-6 text-sm text-muted-foreground"
          data-testid="project-actions-empty"
        >
          <p>
            No actions are published for this project. Read{" "}
            {readability.channelCount} readable channel
            {readability.channelCount === 1 ? "" : "s"}.
          </p>
          <p className="mt-2">
            Publish the agents repository&apos;s{" "}
            <code className="font-mono text-xs">actions.yml</code> with{" "}
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
              tip={tipQuery.data ?? null}
            />
          ))}
        </div>
      )}
    </div>
  );
}
