import * as React from "react";

import type {
  ProjectAgentRow as ProjectAgentRowModel,
  ProjectAgentsModel,
} from "../lib/projectAgentsModel";
import type { ProjectAgentAssociateAccess } from "../lib/publishedProjectAgents";
import type { ProjectAgentsAssignmentScope } from "../lib/useProjectAgents";
import {
  type OpenProjectAgentSession,
  ProjectAgentRow,
} from "./ProjectAgentRow";
import {
  assignmentScopeText,
  countText,
  ASSOCIATION_SCOPE_NOTE,
  HIRER_NOTE,
  PROJECT_AGENTS_EMPTY,
  PROJECT_AGENTS_LOADING,
  PROJECT_AGENTS_SUBTITLE,
  PROJECT_AGENTS_TITLE,
  SECTION_BORROWED,
  SECTION_BORROWED_HINT,
  SECTION_PREVIOUS,
  SECTION_PREVIOUS_HINT,
  SECTION_PROJECT,
  SECTION_PROJECT_HINT,
} from "./projectAgentsCopy";

type RowContext = {
  associateAccess: ProjectAgentAssociateAccess;
  onOpenSession: OpenProjectAgentSession;
  projectId: string;
  projectName: string;
  projectRef: string;
};

function Section({
  context,
  hint,
  rows,
  testId,
  title,
}: {
  context: RowContext;
  hint: string;
  rows: readonly ProjectAgentRowModel[];
  testId: string;
  title: string;
}) {
  if (rows.length === 0) return null;
  return (
    <section className="flex min-w-0 flex-col gap-2" data-testid={testId}>
      <header className="flex min-w-0 flex-col gap-0.5">
        <h3 className="break-words text-sm font-medium text-foreground">
          {title}{" "}
          <span className="text-xs font-normal tabular-nums text-muted-foreground">
            {countText(rows.length, "agent")}
          </span>
        </h3>
        <p className="text-2xs text-muted-foreground">{hint}</p>
      </header>
      <ul className="grid min-w-0 gap-3 lg:grid-cols-2">
        {rows.map((row) => (
          <ProjectAgentRow
            associateAccess={context.associateAccess}
            key={row.pubkey}
            onOpenSession={context.onOpenSession}
            projectId={context.projectId}
            projectName={context.projectName}
            projectRef={context.projectRef}
            row={row}
          />
        ))}
      </ul>
    </section>
  );
}

/**
 * The project Agents tab body, presentational: the hook's answer in, markup
 * out, so a unit test can render every section from a fixture.
 */
export function ProjectAgentsView({
  assignments,
  associateAccess,
  isLoading,
  model,
  notices,
  onOpenSession,
  projectId,
  projectName,
  projectRef,
}: {
  assignments: Pick<
    ProjectAgentsAssignmentScope,
    | "kind"
    | "scannedSessions"
    | "visibleSessions"
    | "message"
    | "hasMore"
    | "isFetchingMore"
    | "fetchMore"
  >;
  /** Whether the viewer may associate an agent with this project. */
  associateAccess: ProjectAgentAssociateAccess;
  isLoading: boolean;
  model: ProjectAgentsModel;
  /** Read limits and failures, each in its own words. */
  notices: readonly string[];
  onOpenSession: OpenProjectAgentSession;
  projectId: string;
  projectName: string;
  /** The project's own address, `30621:<owner>:<d>`. */
  projectRef: string;
}) {
  const total =
    model.projectAgents.length + model.borrowed.length + model.previous.length;
  const scope = assignmentScopeText(assignments);
  const context = React.useMemo<RowContext>(
    () => ({
      associateAccess,
      onOpenSession,
      projectId,
      projectName,
      projectRef,
    }),
    [associateAccess, onOpenSession, projectId, projectName, projectRef],
  );
  return (
    <div className="flex min-w-0 flex-col gap-5" data-testid="project-agents">
      <header className="flex min-w-0 flex-col gap-1">
        <h2 className="text-base font-medium text-foreground">
          {PROJECT_AGENTS_TITLE}
        </h2>
        <p className="text-xs text-muted-foreground">
          {PROJECT_AGENTS_SUBTITLE}
        </p>
      </header>

      {notices.map((notice) => (
        <p
          className="break-words text-xs text-amber-600 dark:text-amber-400"
          data-testid="project-agents-notice"
          key={notice}
        >
          {notice}
        </p>
      ))}

      {total === 0 ? (
        <p
          className="text-sm text-muted-foreground"
          data-testid={
            isLoading ? "project-agents-loading" : "project-agents-empty"
          }
        >
          {isLoading ? PROJECT_AGENTS_LOADING : PROJECT_AGENTS_EMPTY}
        </p>
      ) : null}

      <Section
        context={context}
        hint={SECTION_PROJECT_HINT}
        rows={model.projectAgents}
        testId="project-agents-members"
        title={SECTION_PROJECT}
      />
      <Section
        context={context}
        hint={SECTION_BORROWED_HINT}
        rows={model.borrowed}
        testId="project-agents-borrowed"
        title={SECTION_BORROWED}
      />
      <Section
        context={context}
        hint={SECTION_PREVIOUS_HINT}
        rows={model.previous}
        testId="project-agents-previous"
        title={SECTION_PREVIOUS}
      />

      <footer className="flex min-w-0 flex-col gap-1 break-words text-2xs text-muted-foreground">
        {scope ? (
          <p data-testid="project-agents-assignment-scope">
            {scope}{" "}
            {assignments.hasMore ? (
              <button
                className="underline underline-offset-2 disabled:opacity-50"
                data-testid="project-agents-read-more"
                disabled={assignments.isFetchingMore}
                onClick={assignments.fetchMore}
                type="button"
              >
                {assignments.isFetchingMore ? "Reading…" : "Read more sessions"}
              </button>
            ) : null}
          </p>
        ) : null}
        <p>{ASSOCIATION_SCOPE_NOTE}</p>
        <p>{HIRER_NOTE}</p>
      </footer>
    </div>
  );
}
