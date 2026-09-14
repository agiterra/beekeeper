import { formatAge } from "@/features/roles/ui/rolesCopy";

import type {
  ProjectAgentAssignment,
  ProjectAgentRelationship,
  ProjectAgentSession,
} from "../lib/projectAgentsModel";

/**
 * Every sentence the project Agents tab shows, as values a test can assert.
 *
 * The page answers "who is working here and why" only from records it read:
 * an installation on this computer, a signed execution, a signed assignment.
 * Where one of those reads is limited, the limit is said on the page.
 */

export const PROJECT_AGENTS_TITLE = "Agents";

export const PROJECT_AGENTS_SUBTITLE =
  "Who is working in this project and why: agents installed for it, seated in its sessions, or given one of its assignments.";

export const PROJECT_AGENTS_MISSING = "This project is not readable here.";

export const PROJECT_AGENTS_LOADING = "Reading this project's agents…";

export const PROJECT_AGENTS_EMPTY =
  "No agent is installed for this project, seated in its sessions, or assigned its work.";

export const SECTION_WORKING = "Working here";
export const SECTION_WORKING_HINT =
  "In a session that is still open. Idle agents stay here until the session closes.";
export const SECTION_INSTALLED = "Installed, waiting for a first assignment";
export const SECTION_INSTALLED_HINT =
  "Installed for this project on this computer. No session or assignment names them yet.";
export const SECTION_PREVIOUS = "Previously here";
export const SECTION_PREVIOUS_HINT =
  "Every session they appeared in is closed.";

export const ON_THIS_COMPUTER = "On this computer";

export const SESSIONS_HEADING = "Sessions";
export const ASSIGNMENTS_HEADING = "Assignments";
export const OPEN_SESSION = "Open session";
export const VIEW_INSTRUCTIONS = "Role instructions";
export const ASSIGNMENT_BRIEF = "Brief";
export const ASSIGNMENT_ACCEPTANCE = "Acceptance steps";

/** Each read's limit, said once under the list. */
export const INSTALLATIONS_SCOPE_NOTE =
  "Installations are recorded on the computer that installed them; installs made on another computer are not listed here.";

export const HIRER_NOTE =
  '"Assigned by" comes from a signed assignment. Who granted a seat is not shown: that record is not available to this page.';

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
 * `Builder in Loom session`; installed only, `Installed as Builder`.
 */
export function relationshipText(
  relationship: ProjectAgentRelationship,
): string {
  switch (relationship.kind) {
    case "installed":
      return `Installed as ${titleCaseRole(relationship.role)}`;
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
