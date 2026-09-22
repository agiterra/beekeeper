import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import {
  CODING_SESSION_WORKDIR_STATE_QUERY_KEY,
  getCodingSessionWorkdirState,
} from "@/shared/api/tauriCodingSessionWorkdirs";
import { Button } from "@/shared/ui/button";

import {
  describeAgentsSetup,
  describeCheckoutOutcome,
  describeCodeSeedOutcome,
  describeMigrationOutcome,
  describeRosterOutcome,
  projectAgentsInit,
  type ProjectAgentsInitResult,
} from "../lib/projectAgentsInit";
import {
  ProjectCheckoutFolderField,
  type ProjectCheckoutFolderChoice,
} from "./ProjectCheckoutFolderField";

/**
 * "Create the project's repositories" / "Finish repository setup" — runs
 * `project_agents_init` (spec § 4.11): announce `<slug>` and
 * `<slug>-beekeeper-agents`, seed both, push `main`, set the source, clone
 * the code repository and record it as the project's folder, and put the
 * project's agents on its roster. Idempotent, so the same button finishes a
 * create that stopped part-way. The host's own `complete`/`gap` verdict is
 * what this panel prints.
 *
 * When this computer has no folder recorded for the project, the same
 * "Repository folder" row the create dialog shows sits above the button and
 * its parent is handed to the host; a recorded folder is reused and said
 * so, never re-chosen here (Project settings → This computer owns it).
 */
export function ProjectAgentsInitAction({
  migrateFrom,
  onCancel,
  onRan,
  projectRef,
  projectSlug,
}: {
  /**
   * Set when this project's roles live somewhere else: the source the panel
   * READ, which the host is asked to replace and publishes conditionally
   * on. `null` for an ordinary create or finish.
   */
  migrateFrom?: { eventId: string; repo: string; path: string } | null;
  onCancel: () => void;
  onRan: () => void;
  projectRef: string;
  projectSlug: string;
}) {
  const queryClient = useQueryClient();
  const [pending, setPending] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [result, setResult] = React.useState<ProjectAgentsInitResult | null>(
    null,
  );
  const [checkoutParent, setCheckoutParent] = React.useState<string | null>(
    null,
  );
  const handleCheckoutChange = React.useCallback(
    (choice: ProjectCheckoutFolderChoice) => setCheckoutParent(choice.parent),
    [],
  );
  // The same key every other view renders the workdir store from
  // (item 167); a read here, not a private effect, so a run that records a
  // checkout is one invalidation away from every screen agreeing.
  const workdirQuery = useQuery({
    queryKey: CODING_SESSION_WORKDIR_STATE_QUERY_KEY,
    queryFn: getCodingSessionWorkdirState,
  });
  const recordedPath = workdirQuery.data?.byProject[projectRef]?.path ?? null;
  const askForFolder = workdirQuery.isSuccess && recordedPath === null;

  async function handleRun() {
    setPending(true);
    setError(null);
    try {
      const ran = await projectAgentsInit({
        projectRef,
        checkoutParent: askForFolder ? checkoutParent : null,
        migrate: migrateFrom
          ? { expectedSourceId: migrateFrom.eventId, convert: true }
          : null,
      });
      setResult(ran);
      if (ran.checkoutPath) {
        void queryClient.invalidateQueries({
          queryKey: CODING_SESSION_WORKDIR_STATE_QUERY_KEY,
        });
      }
      onRan();
    } catch (thrown) {
      setError(
        thrown instanceof Error
          ? thrown.message
          : "Failed to create the project's repositories.",
      );
    } finally {
      setPending(false);
    }
  }

  return (
    <div
      className="flex flex-col gap-2 rounded-md border border-border/60 p-3"
      data-testid="project-agents-init-panel"
    >
      {migrateFrom ? (
        <p
          className="text-xs text-muted-foreground"
          data-testid="project-agents-init-migrate-note"
        >
          This project&apos;s roles live in {migrateFrom.repo} at{" "}
          {migrateFrom.path}, which is not its own agents repository. This
          creates {projectSlug}
          -beekeeper-agents, copies those roles into it — each role&apos;s text
          and every skill, unchanged — and points the project at the new
          repository. The repository it points at today is only read; nothing
          there is changed. If someone re-points the project while this runs,
          the move is refused and nothing is overwritten.
        </p>
      ) : null}
      {/* A migration seeds the agents repository from the project's OWN
          roles, so the shipped-templates sentence would contradict the
          paragraph above it. Each state describes only what it does. */}
      <p className="text-xs text-muted-foreground">
        Announces the code repository and the agents repository under your key,
        seeds the code repository with one commit
        {migrateFrom ? null : (
          <>
            {" "}
            and the agents repository from this app&apos;s shipped role
            templates by reference
          </>
        )}{" "}
        (roles/, plans/, each with an archive/), pushes main, sets it as this
        project&apos;s role source, clones the code repository to the folder
        below and records it as this project&apos;s folder, and adds the
        project&apos;s agents to its roster as collaborators. What already
        exists is reused; what is missing is created.
      </p>
      {askForFolder && result === null ? (
        <ProjectCheckoutFolderField
          disabled={pending}
          onChange={handleCheckoutChange}
          slug={projectSlug}
        />
      ) : null}
      {recordedPath ? (
        <p
          className="font-mono text-2xs text-muted-foreground"
          data-testid="project-agents-init-recorded-folder"
        >
          Folder already recorded: {recordedPath}
        </p>
      ) : null}
      {result ? (
        <div
          className="flex flex-col gap-1 text-xs"
          data-testid="project-agents-init-result"
        >
          <p className={result.complete ? "" : "text-destructive"}>
            {describeAgentsSetup(result)}
          </p>
          {result.migratedFrom ? (
            <p
              className={result.sourceConflict ? "text-destructive" : ""}
              data-testid="project-agents-init-migration"
            >
              {describeMigrationOutcome(result)}
            </p>
          ) : null}
          <dl className="grid grid-cols-[auto,1fr] gap-x-3 gap-y-0.5 font-mono text-2xs">
            <dt className="text-muted-foreground">code</dt>
            <dd className="truncate">{result.codeRepoRef}</dd>
            <dt className="text-muted-foreground">code seed</dt>
            <dd
              className="truncate"
              data-testid="project-agents-init-code-seed"
            >
              {describeCodeSeedOutcome(result)}
            </dd>
            <dt className="text-muted-foreground">agents</dt>
            <dd className="truncate">{result.agentsRepoRef}</dd>
            <dt className="text-muted-foreground">seed</dt>
            <dd className="truncate">
              {result.seedCommitSha
                ? `${result.seedCommitSha.slice(0, 8)} (${result.roles.join(", ")})`
                : result.seedSkipped
                  ? "already on the relay"
                  : (result.seedError ?? result.pushError ?? "not reached")}
            </dd>
            <dt className="text-muted-foreground">source</dt>
            <dd className="truncate">
              {result.sourceEventId
                ? result.sourceEventId.slice(0, 8)
                : result.sourceExisted
                  ? "already set"
                  : (result.publicationError ?? "not set")}
            </dd>
            <dt className="text-muted-foreground">agents</dt>
            <dd className="truncate" data-testid="project-agents-init-agents">
              {result.agentsInstalled.length > 0
                ? result.agentsInstalled
                    .map((agent) => `${agent.name} (${agent.role})`)
                    .join(", ")
                : (result.agentsError ?? "none installed")}
            </dd>
            <dt className="text-muted-foreground">folder</dt>
            <dd
              className={result.checkoutPath ? "truncate" : "text-destructive"}
              data-testid="project-agents-init-checkout"
            >
              {describeCheckoutOutcome(result)}
            </dd>
            <dt className="text-muted-foreground">roster</dt>
            <dd
              className={result.rosterError ? "text-destructive" : "truncate"}
              data-testid="project-agents-init-roster"
            >
              {describeRosterOutcome(result)}
            </dd>
          </dl>
        </div>
      ) : null}
      {error ? (
        <p
          className="text-xs text-destructive"
          data-testid="project-agents-init-error"
          role="alert"
        >
          {error}
        </p>
      ) : null}
      <div className="flex gap-2">
        {result?.complete ? null : (
          <Button
            data-testid="project-agents-init-run"
            disabled={pending || workdirQuery.isLoading}
            onClick={() => void handleRun()}
            size="sm"
          >
            {pending ? "Running…" : result ? "Run again" : "Run"}
          </Button>
        )}
        <Button disabled={pending} onClick={onCancel} size="sm" variant="ghost">
          {result ? "Close" : "Cancel"}
        </Button>
      </div>
    </div>
  );
}
