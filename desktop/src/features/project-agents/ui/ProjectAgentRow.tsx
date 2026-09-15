import * as React from "react";
import { Link } from "@tanstack/react-router";

import {
  AgentDirectoryRenameButton,
  AgentDirectoryRenameForm,
} from "@/features/agents/ui/AgentDirectoryRename";
import {
  DOT_DEPLOYED,
  DOT_MUTED,
  DOT_RUNNING,
  DOT_WAITING,
  seatStatusDotClass,
  StatusDot,
} from "@/features/roles/ui/roleDots";
import { truncatePubkey } from "@/shared/lib/pubkey";
import { Button } from "@/shared/ui/button";
import { UserAvatar } from "@/shared/ui/UserAvatar";

import type {
  ProjectAgentAssignment,
  ProjectAgentOpenTarget,
  ProjectAgentRow as ProjectAgentRowModel,
  ProjectAgentSession,
  ProjectAgentState,
} from "../lib/projectAgentsModel";
import type { ProjectAgentAssociateAccess } from "../lib/publishedProjectAgents";
import { ProjectAgentAssociate } from "./ProjectAgentAssociate";
import {
  ASSIGNMENT_ACCEPTANCE,
  ASSIGNMENT_BRIEF,
  ASSIGNMENTS_HEADING,
  ASSOCIATION_MISSING_WARNING,
  assignmentByText,
  assignmentStatusText,
  BADGE_BORROWED,
  BADGE_NOT_ASSOCIATED_HERE,
  BADGE_PREVIOUS,
  BADGE_PROJECT_AGENT,
  BADGE_UNVERIFIED,
  carriedAssociationText,
  countText,
  DETAILS_HEADING,
  elsewhereText,
  installationText,
  LOCATION_UNKNOWN,
  lastSeenText,
  notProjectAgentText,
  ON_THIS_COMPUTER,
  OPEN_SESSION,
  primaryRoleText,
  relationshipText,
  SESSIONS_HEADING,
  seatedRolesText,
  sessionEngineText,
  sessionInstructionsText,
  sessionStatusText,
  stateText,
  unverifiedClaimText,
  VIEW_INSTRUCTIONS,
} from "./projectAgentsCopy";

const DETAILS_SUMMARY_CLASS =
  "cursor-pointer text-xs text-muted-foreground marker:text-muted-foreground/60";

const BADGE_CLASS =
  "shrink-0 rounded-full border border-border px-1.5 py-px text-2xs text-muted-foreground";

function stateDotClass(state: ProjectAgentState): string {
  switch (state) {
    case "working":
      return DOT_RUNNING;
    case "idle":
      return DOT_DEPLOYED;
    case "disconnected":
      return DOT_WAITING;
    default:
      return DOT_MUTED;
  }
}

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
      <div className="flex min-w-0 flex-wrap items-center gap-x-1.5 gap-y-0.5">
        <StatusDot
          className={seatStatusDotClass(session.status)}
          title={session.status}
        />
        <span
          className="min-w-0 truncate text-sm text-foreground"
          title={session.sessionName}
        >
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
      <p className="break-words text-xs text-muted-foreground">
        {sessionEngineText(session)} · {sessionStatusText(session)}
      </p>
      <p
        className="break-words text-xs text-muted-foreground"
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
          title={`${session.providerAuthorityPubkey} — the provider key that signs this session's records, the only machine identity the session carries.`}
        >
          host {truncatePubkey(session.providerAuthorityPubkey)}
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

function sectionBadges(row: ProjectAgentRowModel): string[] {
  if (row.section === "project") return [BADGE_PROJECT_AGENT];
  if (row.section === "unverified") return [BADGE_UNVERIFIED];
  if (row.section === "borrowed") return [BADGE_BORROWED];
  if (row.section === "available") return [BADGE_NOT_ASSOCIATED_HERE];
  return [
    BADGE_PREVIOUS,
    row.isProjectAgent ? BADGE_PROJECT_AGENT : BADGE_BORROWED,
  ];
}

function LocationText({ row }: { row: ProjectAgentRowModel }) {
  const { location } = row;
  if (location.kind === "here") {
    return <span className="shrink-0">{ON_THIS_COMPUTER}</span>;
  }
  if (location.kind === "elsewhere") {
    return (
      <span
        className="min-w-0 break-words"
        data-testid="project-agent-owner"
        title={location.ownerPubkey}
      >
        {elsewhereText(location.ownerName)}
      </span>
    );
  }
  return <span className="shrink-0">{LOCATION_UNKNOWN}</span>;
}

/** Why a row that is not a project agent here is not one, in words. */
function MembershipNote({
  projectName,
  row,
}: {
  projectName: string;
  row: ProjectAgentRowModel;
}) {
  if (row.isProjectAgent) return null;
  if (row.section === "unverified") {
    const owner =
      row.location.kind === "elsewhere" ? row.location.ownerName : "its owner";
    return (
      <p
        className="break-words text-xs text-amber-700 dark:text-amber-400"
        data-testid="project-agent-unverified"
        role="note"
      >
        {unverifiedClaimText(projectName, owner)}
      </p>
    );
  }
  if (row.carriedFromAnotherComputer) {
    return (
      <p
        className="break-words text-xs text-muted-foreground"
        data-testid="project-agent-carried"
      >
        {carriedAssociationText(projectName)}
      </p>
    );
  }
  return (
    <p
      className="break-words text-xs text-muted-foreground"
      data-testid="project-agent-not-member"
    >
      {notProjectAgentText(projectName, row.otherProject)}
    </p>
  );
}

/** One agent identity in a project, with its sessions and assignments beneath. */
export function ProjectAgentRow({
  associateAccess,
  onOpenSession,
  projectId,
  projectName,
  projectRef,
  row,
}: {
  associateAccess: ProjectAgentAssociateAccess;
  onOpenSession: OpenProjectAgentSession;
  projectId: string;
  projectName: string;
  projectRef: string;
  row: ProjectAgentRowModel;
}) {
  const [isRenaming, setIsRenaming] = React.useState(false);
  const canRename = row.managedHere;
  const seatedText =
    row.section === "project" ? null : seatedRolesText(row.seatedRoles);
  return (
    <li
      className="flex min-w-0 flex-col gap-2 rounded-lg border border-border bg-card p-3"
      data-agent-pubkey={row.pubkey}
      data-agent-section={row.section}
      data-agent-state={row.state}
      data-testid="project-agent-row"
    >
      <div className="flex min-w-0 flex-wrap items-start gap-2">
        <UserAvatar
          avatarUrl={row.avatarUrl}
          displayName={row.name}
          fallbackDelayMs={0}
          size="sm"
        />
        <div className="flex min-w-0 flex-1 basis-48 flex-col gap-0.5">
          {isRenaming ? (
            <AgentDirectoryRenameForm
              name={row.name}
              onDone={() => setIsRenaming(false)}
              pubkey={row.pubkey}
            />
          ) : (
            <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5">
              <span
                className="min-w-0 truncate text-sm font-medium text-foreground"
                data-testid="project-agent-name"
                title={`${row.name} · ${row.pubkey}`}
              >
                {row.name}
              </span>
              {sectionBadges(row).map((badge) => (
                <span
                  className={BADGE_CLASS}
                  data-testid="project-agent-badge"
                  key={badge}
                >
                  {badge}
                </span>
              ))}
            </div>
          )}
          <p className="flex min-w-0 flex-wrap items-center gap-x-1.5 gap-y-0.5 text-xs text-muted-foreground">
            <span
              className="shrink-0 text-foreground/90"
              data-testid="project-agent-role"
            >
              {primaryRoleText(row.primaryRole)}
            </span>
            <span aria-hidden>·</span>
            <span
              className="flex shrink-0 items-center gap-1"
              data-testid="project-agent-state"
            >
              <StatusDot
                className={stateDotClass(row.state)}
                title={stateText(row.state)}
              />
              {stateText(row.state)}
            </span>
            <span aria-hidden>·</span>
            <LocationText row={row} />
          </p>
          {row.relationship ? (
            <p
              className="break-words text-sm text-foreground/90"
              data-testid="project-agent-relationship"
            >
              {relationshipText(row.relationship)}
            </p>
          ) : null}
          {seatedText ? (
            <p className="break-words text-xs text-muted-foreground">
              {seatedText}
            </p>
          ) : null}
          {row.associationMissing ? (
            <p
              className="break-words text-xs text-amber-700 dark:text-amber-400"
              data-testid="project-agent-association-missing"
              role="note"
            >
              {ASSOCIATION_MISSING_WARNING}
            </p>
          ) : null}
          <MembershipNote projectName={projectName} row={row} />
          <p className="break-words text-xs text-muted-foreground">
            {lastSeenText(row.lastSeenSeconds)}
            {row.primaryRole || row.seatedRoles.length > 0 ? (
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
          <div className="shrink-0">
            <AgentDirectoryRenameButton
              name={row.name}
              onClick={() => setIsRenaming(true)}
            />
          </div>
        ) : null}
      </div>

      {row.mayAssociate && row.primaryRole ? (
        <ProjectAgentAssociate
          access={associateAccess}
          name={row.name}
          projectName={projectName}
          projectRef={projectRef}
          pubkey={row.pubkey}
          role={row.primaryRole}
        />
      ) : null}

      <details data-testid="project-agent-details">
        <summary className={DETAILS_SUMMARY_CLASS}>{DETAILS_HEADING}</summary>
        <div className="mt-1 flex min-w-0 flex-col gap-0.5">
          <p
            className="truncate font-mono text-2xs text-muted-foreground"
            data-testid="project-agent-pubkey"
            title={row.pubkey}
          >
            {row.pubkey}
          </p>
          {row.installations.map((installation) => (
            <p
              className="break-words text-xs text-muted-foreground"
              data-testid="project-agent-installation"
              key={installation.role}
              title={`${installation.packRef.repo} ${installation.packRef.path} @ ${installation.packRef.sha}`}
            >
              {installationText(installation.role, installation.packRef.sha)}
            </p>
          ))}
        </div>
      </details>

      {row.sessions.length > 0 ? (
        <details
          data-testid="project-agent-sessions"
          open={row.state === "working"}
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
