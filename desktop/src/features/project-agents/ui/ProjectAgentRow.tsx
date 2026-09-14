import * as React from "react";
import { Link } from "@tanstack/react-router";

import {
  AgentDirectoryRenameButton,
  AgentDirectoryRenameForm,
} from "@/features/agents/ui/AgentDirectoryRename";
import { seatStatusDotClass, StatusDot } from "@/features/roles/ui/roleDots";
import { truncatePubkey } from "@/shared/lib/pubkey";
import { Button } from "@/shared/ui/button";
import { UserAvatar } from "@/shared/ui/UserAvatar";

import type {
  ProjectAgentAssignment,
  ProjectAgentOpenTarget,
  ProjectAgentRow as ProjectAgentRowModel,
  ProjectAgentSession,
} from "../lib/projectAgentsModel";
import {
  ASSIGNMENT_ACCEPTANCE,
  ASSIGNMENT_BRIEF,
  ASSIGNMENTS_HEADING,
  assignmentByText,
  assignmentStatusText,
  countText,
  installationText,
  lastSeenText,
  ON_THIS_COMPUTER,
  OPEN_SESSION,
  relationshipText,
  SESSIONS_HEADING,
  sessionEngineText,
  sessionInstructionsText,
  sessionStatusText,
  VIEW_INSTRUCTIONS,
} from "./projectAgentsCopy";

const DETAILS_SUMMARY_CLASS =
  "cursor-pointer text-xs text-muted-foreground marker:text-muted-foreground/60";

export type OpenProjectAgentSession = (
  target: ProjectAgentOpenTarget,
  sessionRef: string | null,
) => void;

function SessionItem({
  onOpen,
  session,
}: {
  onOpen: OpenProjectAgentSession;
  session: ProjectAgentSession;
}) {
  return (
    <li
      className="flex min-w-0 flex-col gap-0.5 rounded-md border border-border/60 px-2 py-1.5"
      data-session-closed={session.sessionClosed ? "true" : "false"}
      data-testid="project-agent-session"
    >
      <div className="flex min-w-0 items-center gap-1.5">
        <StatusDot
          className={seatStatusDotClass(session.status)}
          title={session.status}
        />
        <span className="truncate text-sm text-foreground">
          {session.sessionName}
        </span>
        {session.role ? (
          <span className="shrink-0 font-mono text-2xs text-muted-foreground">
            {session.role}
          </span>
        ) : null}
        <Button
          className="ml-auto h-6 shrink-0 px-2 text-xs"
          data-testid="project-agent-open-session"
          onClick={() => onOpen(session.openTarget, session.sessionRef)}
          size="sm"
          type="button"
          variant="ghost"
        >
          {OPEN_SESSION}
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">
        {sessionEngineText(session)} · {sessionStatusText(session)}
      </p>
      <p
        className="text-xs text-muted-foreground"
        data-pack-differs={session.packDiffersFromInstalled ? "true" : "false"}
        data-testid="project-agent-session-instructions"
        title={
          session.packRef
            ? `${session.packRef.repo} ${session.packRef.path} @ ${session.packRef.sha}`
            : undefined
        }
      >
        {sessionInstructionsText(session)}
      </p>
      {session.providerAuthorityPubkey ? (
        <p
          className="truncate font-mono text-2xs text-muted-foreground/80"
          title="The provider key that signs this session's records — the only machine identity the session carries."
        >
          provider {truncatePubkey(session.providerAuthorityPubkey)}
        </p>
      ) : null}
    </li>
  );
}

function AssignmentItem({
  assignment,
  onOpen,
}: {
  assignment: ProjectAgentAssignment;
  onOpen: OpenProjectAgentSession;
}) {
  const target = assignment.openTarget;
  return (
    <li
      className="flex min-w-0 flex-col gap-0.5 rounded-md border border-border/60 px-2 py-1.5"
      data-assignment-status={assignment.status}
      data-testid="project-agent-assignment"
    >
      <div className="flex min-w-0 items-start gap-1.5">
        <p className="min-w-0 flex-1 text-sm text-foreground">
          {assignment.objective}
        </p>
        {target ? (
          <Button
            className="h-6 shrink-0 px-2 text-xs"
            onClick={() => onOpen(target, assignment.sessionRef)}
            size="sm"
            type="button"
            variant="ghost"
          >
            {OPEN_SESSION}
          </Button>
        ) : null}
      </div>
      <p className="text-xs text-muted-foreground">
        {assignmentByText(assignment)} · {assignmentStatusText(assignment)}
      </p>
      {assignment.brief || assignment.acceptanceSteps.length > 0 ? (
        <details data-testid="project-agent-assignment-brief">
          <summary className={DETAILS_SUMMARY_CLASS}>
            {ASSIGNMENT_BRIEF}
          </summary>
          <div className="mt-1 flex flex-col gap-1">
            {assignment.brief ? (
              <p className="max-h-64 overflow-y-auto whitespace-pre-wrap text-xs text-muted-foreground">
                {assignment.brief}
              </p>
            ) : null}
            {assignment.acceptanceSteps.length > 0 ? (
              <>
                <p className="text-xs font-medium text-foreground">
                  {ASSIGNMENT_ACCEPTANCE}
                </p>
                <ol className="list-decimal pl-5 text-xs text-muted-foreground">
                  {assignment.acceptanceSteps.map((step, index) => (
                    // Steps are an ordered signed list; position is their identity.
                    // biome-ignore lint/suspicious/noArrayIndexKey: ordered signed list
                    <li key={index}>{step}</li>
                  ))}
                </ol>
              </>
            ) : null}
          </div>
        </details>
      ) : null}
    </li>
  );
}

/** One agent identity in a project, with its sessions and assignments beneath. */
export function ProjectAgentRow({
  onOpenSession,
  projectId,
  row,
}: {
  onOpenSession: OpenProjectAgentSession;
  projectId: string;
  row: ProjectAgentRowModel;
}) {
  const [isRenaming, setIsRenaming] = React.useState(false);
  const canRename = row.managedHere;
  return (
    <li
      className="flex min-w-0 flex-col gap-2 rounded-lg border border-border bg-card p-3"
      data-agent-pubkey={row.pubkey}
      data-agent-section={row.section}
      data-testid="project-agent-row"
    >
      <div className="flex min-w-0 items-start gap-2">
        <UserAvatar
          avatarUrl={row.avatarUrl}
          displayName={row.name}
          fallbackDelayMs={0}
          size="sm"
        />
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          {isRenaming ? (
            <AgentDirectoryRenameForm
              name={row.name}
              onDone={() => setIsRenaming(false)}
              pubkey={row.pubkey}
            />
          ) : (
            <div className="flex min-w-0 items-baseline gap-2">
              <span
                className="truncate text-sm font-medium text-foreground"
                data-testid="project-agent-name"
                title={row.pubkey}
              >
                {row.name}
              </span>
              {row.managedHere ? (
                <span className="shrink-0 text-2xs text-muted-foreground">
                  {ON_THIS_COMPUTER}
                </span>
              ) : null}
            </div>
          )}
          {row.relationship.kind === "installed" ? null : (
            // An installed-only row's installation line below already says
            // everything its relationship sentence would.
            <p
              className="text-sm text-foreground/90"
              data-testid="project-agent-relationship"
            >
              {relationshipText(row.relationship)}
            </p>
          )}
          {row.installations.map((installation) => (
            <p
              className="text-xs text-muted-foreground"
              data-testid="project-agent-installation"
              key={installation.role}
              title={`${installation.packRef.repo} ${installation.packRef.path} @ ${installation.packRef.sha}`}
            >
              {installationText(installation.role, installation.packRef.sha)}
            </p>
          ))}
          <p className="text-xs text-muted-foreground">
            {lastSeenText(row.lastSeenSeconds)}
            {row.roles.length > 0 ? (
              <>
                {" · "}
                <Link
                  className="underline-offset-2 hover:underline"
                  data-testid="project-agent-instructions-link"
                  params={{ projectId }}
                  to="/projects/$projectId/packs"
                >
                  {VIEW_INSTRUCTIONS}
                </Link>
              </>
            ) : null}
          </p>
        </div>
        {canRename && !isRenaming ? (
          <AgentDirectoryRenameButton
            name={row.name}
            onClick={() => setIsRenaming(true)}
          />
        ) : null}
      </div>

      {row.sessions.length > 0 ? (
        <details
          data-testid="project-agent-sessions"
          open={row.section === "working"}
        >
          <summary className={DETAILS_SUMMARY_CLASS}>
            {SESSIONS_HEADING} ({countText(row.sessions.length, "session")})
          </summary>
          <ul className="mt-1 flex flex-col gap-1">
            {row.sessions.map((session) => (
              <SessionItem
                key={session.key}
                onOpen={onOpenSession}
                session={session}
              />
            ))}
          </ul>
        </details>
      ) : null}

      {row.assignments.length > 0 ? (
        <details data-testid="project-agent-assignments">
          <summary className={DETAILS_SUMMARY_CLASS}>
            {ASSIGNMENTS_HEADING} (
            {countText(row.assignments.length, "assignment")})
          </summary>
          <ul className="mt-1 flex flex-col gap-1">
            {row.assignments.map((assignment) => (
              <AssignmentItem
                assignment={assignment}
                key={assignment.key}
                onOpen={onOpenSession}
              />
            ))}
          </ul>
        </details>
      ) : null}
    </li>
  );
}
