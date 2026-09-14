import type {
  ProjectAgentRow as ProjectAgentRowModel,
  ProjectAgentsModel,
} from "../lib/projectAgentsModel";
import type { ProjectAgentsAssignmentScope } from "../lib/useProjectAgents";
import {
  type OpenProjectAgentSession,
  ProjectAgentRow,
} from "./ProjectAgentRow";
import {
  assignmentScopeText,
  countText,
  HIRER_NOTE,
  INSTALLATIONS_SCOPE_NOTE,
  PROJECT_AGENTS_EMPTY,
  PROJECT_AGENTS_LOADING,
  PROJECT_AGENTS_SUBTITLE,
  PROJECT_AGENTS_TITLE,
  SECTION_INSTALLED,
  SECTION_INSTALLED_HINT,
  SECTION_PREVIOUS,
  SECTION_PREVIOUS_HINT,
  SECTION_WORKING,
  SECTION_WORKING_HINT,
} from "./projectAgentsCopy";

function Section({
  hint,
  onOpenSession,
  projectId,
  rows,
  testId,
  title,
}: {
  hint: string;
  onOpenSession: OpenProjectAgentSession;
  projectId: string;
  rows: readonly ProjectAgentRowModel[];
  testId: string;
  title: string;
}) {
  if (rows.length === 0) return null;
  return (
    <section className="flex flex-col gap-2" data-testid={testId}>
      <header className="flex flex-col gap-0.5">
        <h3 className="text-sm font-medium text-foreground">
          {title}{" "}
          <span className="text-xs font-normal tabular-nums text-muted-foreground">
            {countText(rows.length, "agent")}
          </span>
        </h3>
        <p className="text-2xs text-muted-foreground">{hint}</p>
      </header>
      <ul className="grid gap-3 lg:grid-cols-2">
        {rows.map((row) => (
          <ProjectAgentRow
            key={row.pubkey}
            onOpenSession={onOpenSession}
            projectId={projectId}
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
  isLoading,
  model,
  notices,
  onOpenSession,
  projectId,
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
  isLoading: boolean;
  model: ProjectAgentsModel;
  /** Read limits and failures, each in its own words. */
  notices: readonly string[];
  onOpenSession: OpenProjectAgentSession;
  projectId: string;
}) {
  const total =
    model.working.length + model.installed.length + model.previous.length;
  const scope = assignmentScopeText(assignments);
  return (
    <div className="flex flex-col gap-5" data-testid="project-agents">
      <header className="flex flex-col gap-1">
        <h2 className="text-base font-medium text-foreground">
          {PROJECT_AGENTS_TITLE}
        </h2>
        <p className="text-xs text-muted-foreground">
          {PROJECT_AGENTS_SUBTITLE}
        </p>
      </header>

      {notices.map((notice) => (
        <p
          className="text-xs text-amber-600 dark:text-amber-400"
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
        hint={SECTION_WORKING_HINT}
        onOpenSession={onOpenSession}
        projectId={projectId}
        rows={model.working}
        testId="project-agents-working"
        title={SECTION_WORKING}
      />
      <Section
        hint={SECTION_INSTALLED_HINT}
        onOpenSession={onOpenSession}
        projectId={projectId}
        rows={model.installed}
        testId="project-agents-installed"
        title={SECTION_INSTALLED}
      />
      <Section
        hint={SECTION_PREVIOUS_HINT}
        onOpenSession={onOpenSession}
        projectId={projectId}
        rows={model.previous}
        testId="project-agents-previous"
        title={SECTION_PREVIOUS}
      />

      <footer className="flex flex-col gap-1 text-2xs text-muted-foreground">
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
        <p>{INSTALLATIONS_SCOPE_NOTE}</p>
        <p>{HIRER_NOTE}</p>
      </footer>
    </div>
  );
}
