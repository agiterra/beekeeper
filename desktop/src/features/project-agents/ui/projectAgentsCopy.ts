import { formatCodingSessionRuntimeLabel } from "@/features/coding-sessions/lib/codingSessionLabels";
import { formatAge } from "@/features/roles/ui/rolesCopy";

import type {
  ProjectAgentAssignment,
  ProjectAgentsReadinessLine,
  ProjectAgentRelationship,
  ProjectAgentRow,
  ProjectAgentState,
  ProjectAgentSession,
} from "../lib/projectAgentsModel";

/**
 * Every sentence the project Agents tab shows, as values a test can assert.
 *
 * The page answers "who belongs to this project, with what primary role,
 * doing what" from association (a managed record here, or an authorized
 * publication) and shows everyone else who took part as a participant, never
 * as a member. Where a read is limited, the limit is said on the page.
 */

export const PROJECT_AGENTS_TITLE = "Agents";

export const PROJECT_AGENTS_SUBTITLE =
  "Who belongs to this project, with what primary role, and what they are doing. Seats and past work do not make an agent a member.";

export const PROJECT_AGENTS_MISSING = "This project is not readable here.";

export const PROJECT_AGENTS_LOADING = "Reading this project's agents…";

export const PROJECT_AGENTS_EMPTY =
  "No agent is associated with this project, and nobody has worked in its sessions.";

export const SECTION_PROJECT = "Project agents";
export const SECTION_PROJECT_HINT =
  "Associated with this project. The lead hires only these, each in its primary role.";
export const SECTION_BORROWED = "Borrowed participants";
export const SECTION_BORROWED_HINT =
  "Seated or assigned in a session that is still open, without being this project's agents.";
export const SECTION_UNVERIFIED = "Project authority not verified";
export const SECTION_UNVERIFIED_HINT =
  "Published as this project's agents, but its members could not be read to confirm the publisher may associate agents. Not counted as project agents.";
export const SECTION_AVAILABLE = "Available to associate";
export const SECTION_AVAILABLE_HINT =
  "On this computer and published as this project's agents from another of your computers. Not project agents here until associated.";
export const SECTION_PREVIOUS = "Previously here";
export const SECTION_PREVIOUS_HINT =
  "Every session they appeared in is closed. Their work stays attributed to them.";

export const BADGE_PROJECT_AGENT = "Project agent";
export const BADGE_BORROWED = "Borrowed";
export const BADGE_PREVIOUS = "Previously here";
export const BADGE_UNVERIFIED = "Project authority not verified";
export const BADGE_NOT_ASSOCIATED_HERE = "Not associated here";

/** Said under Project agents for a private project, whose agents are never published. */
export const PROJECT_PRIVATE_NOTE =
  "This project is private, so agents on other computers are not published. Only this computer's agents are listed.";

export const ON_THIS_COMPUTER = "On this computer";
export const LOCATION_UNKNOWN = "Not on this computer";

export const SESSIONS_HEADING = "Sessions";
export const ASSIGNMENTS_HEADING = "Assignments";
export const DETAILS_HEADING = "Details";
export const OPEN_SESSION = "Open session";
export const VIEW_INSTRUCTIONS = "Role instructions";
export const ASSIGNMENT_BRIEF = "Brief";
export const ASSIGNMENT_ACCEPTANCE = "Acceptance steps";

/** Each read's limit, said once under the list. */
export const ASSOCIATION_SCOPE_NOTE =
  "Agents on other computers are listed when their owner is this project's creator, owner or collaborator and has published the association.";

export const HIRER_NOTE =
  '"Assigned by" comes from a signed assignment. Who granted a seat is not shown: that record is not available to this page.';

const STATE_WORDS: Record<ProjectAgentState, string> = {
  working: "Working",
  idle: "Idle",
  disconnected: "Disconnected",
  available: "Available",
  "not-associated": "Not associated yet",
  elsewhere: "On another computer",
  carried: "Associated from another computer",
  "not-running": "Not running",
  historical: "Historical",
};

/** The row's state as a word — never colour alone. */
export function stateText(state: ProjectAgentState): string {
  return STATE_WORDS[state];
}

/** `Builder`, or `No primary role`. */
export function primaryRoleText(role: string | null): string {
  return role ? titleCaseRole(role) : "No primary role";
}

/**
 * `Claude Code`, `Claude Code · opus`, or `runtime not set` when the agent
 * genuinely has none — the same runtime display name the dashboard's harness
 * catalog and coding sessions use, never a bespoke label.
 *
 * Reads `effectiveRuntime`, not the raw `runtime`. Before ledger 139 this
 * read `runtime` alone, so any agent that inherits its harness from its
 * persona — Kiln, on Codex — printed "runtime not set" on the project Agents
 * tab even while it ran and hired correctly (ledger 135(d)): the raw field is
 * blank by design for an inheriting agent, `effectiveRuntime` is not.
 */
export function runtimeText(
  agent: Pick<ProjectAgentRow, "effectiveRuntime" | "model">,
): string {
  if (!agent.effectiveRuntime) return "runtime not set";
  const label = formatCodingSessionRuntimeLabel(agent.effectiveRuntime);
  return agent.model ? `${label} · ${agent.model}` : label;
}

/** `Owned by Andy · can't run on this computer`. */
export function elsewhereText(ownerName: string): string {
  return `Owned by ${ownerName} · can't run on this computer`;
}

/**
 * A published claim whose author's project authority was not verified.
 * `Published as a Tank Loop agent by Andy. Tank Loop's members could not be
 * read, so Andy's authority to associate agents is not verified.`
 */
export function unverifiedClaimText(
  projectName: string,
  ownerName: string,
): string {
  return `Published as a ${projectName} agent by ${ownerName}. ${projectName}'s members could not be read, so ${ownerName}'s authority to associate agents is not verified.`;
}

/** A local agent carrying this project's digest from another computer. */
export function carriedAssociationText(projectName: string): string {
  return `Published as a ${projectName} agent from another of your computers. Associate it here to let this computer hire it.`;
}

/** `Bob`, `Bob and Ira`, `Bob, Gordan and Ira`. */
export function namesText(names: readonly string[]): string {
  if (names.length <= 1) return names[0] ?? "";
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

/**
 * One role's hiring-readiness line. Names come from the evidence and are
 * never offered as a choice.
 */
export function readinessText(
  line: ProjectAgentsReadinessLine,
  projectName: string,
): string {
  const { role } = line;
  const head = `No ${role} agent belongs to ${projectName} on this computer.`;
  const tail = `A lead's ${role} hires will be refused until you associate an agent.`;
  if (line.workers.length === 0) return `${head} ${tail}`;
  const names = namesText(line.workers.map((worker) => worker.name));
  return `${head} Past ${role} work here was done by ${names} (not associated). ${tail}`;
}

/** Said on a project row the lead cannot hire. */
export const ASSOCIATION_MISSING_WARNING =
  "Installed for this project but not associated yet — the lead can't hire it. Reopen setup or associate it.";

/**
 * What a borrowed or previous participant is not. An agent with no
 * association and one that belongs to another project are different facts.
 */
export function notProjectAgentText(
  projectName: string,
  otherProject: { name: string | null } | null,
): string {
  if (otherProject) {
    const other = otherProject.name ?? "another project";
    return `Not a ${projectName} agent — belongs to ${other}. New hires use this project's agents only.`;
  }
  return `Not a ${projectName} agent — seated here without project association. New hires use this project's agents only.`;
}

export function associateLabel(projectName: string): string {
  return `Associate with ${projectName}`;
}

export function associateConfirmText(
  name: string,
  projectName: string,
  role: string,
): string {
  return `${name} becomes a permanent ${projectName} ${titleCaseRole(role)} agent. Its history stays attributed to ${name}. This does not change project access.`;
}

export const ASSOCIATE_CONFIRM = "Associate";
export const ASSOCIATE_CANCEL = "Cancel";
export const ASSOCIATE_PENDING = "Associating…";

/** `Seated as Builder, Verifier`. */
export function seatedRolesText(roles: readonly string[]): string | null {
  if (roles.length === 0) return null;
  return `Seated as ${roles.map(titleCaseRole).join(", ")}`;
}

export function titleCaseRole(role: string): string {
  return role
    .split("-")
    .filter((part) => part.length > 0)
    .map((part) => part[0].toUpperCase() + part.slice(1))
    .join(" ");
}

/**
 * The row's lead sentence.
 *
 * `Builder in Loom session · assigned by Loom`; without an assignment,
 * `Builder in Loom session`.
 */
export function relationshipText(
  relationship: ProjectAgentRelationship,
): string {
  switch (relationship.kind) {
    case "assigned":
      return `Assigned as ${titleCaseRole(relationship.role)} in ${relationship.sessionName} · by ${relationship.assignerName}`;
    case "seated": {
      const role = relationship.role
        ? titleCaseRole(relationship.role)
        : "Seated";
      const base = `${role} in ${relationship.sessionName}`;
      return relationship.assignerName
        ? `${base} · assigned by ${relationship.assignerName}`
        : base;
    }
  }
}

/** First eight characters of a 40-hex sha; an app version string stays whole. */
export function shortPackSha(sha: string): string {
  return /^[0-9a-f]{40}$/.test(sha) ? sha.slice(0, 8) : sha;
}

export function installationText(role: string, sha: string): string {
  return `Installed as ${titleCaseRole(role)} · instructions ${shortPackSha(sha)}`;
}

/** `claude-primary · sonnet`, or the runtime, or nothing reported. */
export function sessionEngineText(
  session: Pick<ProjectAgentSession, "provider" | "runtime" | "model">,
): string {
  const engine = session.provider ?? session.runtime;
  if (engine && session.model) return `${engine} · ${session.model}`;
  return engine ?? session.model ?? "runtime not reported";
}

/** `idle (3h)`, `idle (3h) · session closed`. */
export function sessionStatusText(
  session: Pick<ProjectAgentSession, "status" | "ageSeconds" | "sessionClosed">,
): string {
  const status = `${session.status} (${formatAge(session.ageSeconds)})`;
  return session.sessionClosed ? `${status} · session closed` : status;
}

/** `builder @ f0132d13`, a mismatch note, or the disclosed absence. */
export function sessionInstructionsText(
  session: Pick<ProjectAgentSession, "packRef" | "packDiffersFromInstalled">,
): string {
  if (!session.packRef) return "Instructions revision not reported";
  const base = `Instructions ${session.packRef.role} @ ${shortPackSha(session.packRef.sha)}`;
  return session.packDiffersFromInstalled
    ? `${base} · differs from the revision installed here`
    : base;
}

const ASSIGNMENT_STATUS_WORDS: Record<
  ProjectAgentAssignment["status"],
  string
> = {
  unresolved: "no report yet",
  reported: "reported, awaiting a ruling",
  settled: "settled",
};

/** `settled · approve-with-notes · 2 reports`. */
export function assignmentStatusText(
  assignment: Pick<
    ProjectAgentAssignment,
    "status" | "latestDecision" | "reportCount"
  >,
): string {
  const parts = [ASSIGNMENT_STATUS_WORDS[assignment.status]];
  if (assignment.latestDecision) parts.push(assignment.latestDecision);
  if (assignment.reportCount > 0) {
    parts.push(
      `${assignment.reportCount} ${assignment.reportCount === 1 ? "report" : "reports"}`,
    );
  }
  return parts.join(" · ");
}

export function assignmentByText(
  assignment: Pick<ProjectAgentAssignment, "assignerName" | "sessionName">,
): string {
  return `Assigned by ${assignment.assignerName} in ${assignment.sessionName}`;
}

/** `Last seen 3h ago`, or nothing observed. */
export function lastSeenText(seconds: number | null): string {
  if (seconds === null) return "No status observed";
  const age = formatAge(seconds);
  return age === "just now" ? "Last seen just now" : `Last seen ${age} ago`;
}

export function countText(count: number, noun: string): string {
  return `${count} ${count === 1 ? noun : `${noun}s`}`;
}

/**
 * The assignment read's scope: it opens a project's sessions eight at a time,
 * newest first. Unread sessions are counted, never rendered as "no work".
 */
export function assignmentScopeText(scope: {
  kind: "loading" | "ready" | "unreadable";
  scannedSessions: number;
  visibleSessions: number;
  message: string | null;
}): string | null {
  if (scope.kind === "loading") return "Reading assignments…";
  if (scope.kind === "unreadable") {
    return `Assignments could not be read${scope.message ? `: ${scope.message}` : "."}`;
  }
  if (scope.scannedSessions < scope.visibleSessions) {
    return `Assignments read from the newest ${scope.scannedSessions} of ${scope.visibleSessions} sessions.`;
  }
  return null;
}
