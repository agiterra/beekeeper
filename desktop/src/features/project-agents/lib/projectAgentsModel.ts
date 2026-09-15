/**
 * The project Agents tab — who belongs to one project, with what primary
 * role, doing what (`docs/PROJECT_AGENT_HIRING_IMPL.md` § UI rules).
 *
 * **Membership is association, never a seat.** An identity is a project
 * agent when
 *
 * - its managed record on this computer has `projectRef` equal to the
 *   project, or
 * - it has no record here and a kind:30177 from an authorized author (the
 *   creator, a roster owner or collaborator) carries this project's digest
 *   (`publishedProjectAgents.ts` filters those before they arrive here).
 *
 * The local record wins for an agent held here: a stale publication cannot
 * put back an agent this computer has not associated.
 *
 * A setup journal is not membership. An agent the journal installed for this
 * project whose record lacks the association is listed under Project agents
 * **with a warning**, because the lead cannot hire it — saying nothing would
 * hide exactly why a hire refused.
 *
 * Everyone else with signed evidence in the project is a participant, not a
 * member: **Borrowed** while a session they appear in is open, **Previously
 * here** when every such session is closed. A project agent with closed
 * history stays a project agent; that history is in its details.
 *
 * State comes from the newest execution in an *open* session, never from an
 * open session alone: a stopped or finished execution in an open session is
 * not live, and a closed session's last status is not a present tense.
 */
import type { PackRef } from "@/features/coding-sessions/lib/codingSessionPackRef";
import type { CodingSessionStatus } from "@/features/coding-sessions/lib/codingSessionTypes";
import {
  normalizeProjectCoordinate,
  projectAgentDigest,
  type PublishedAgentAssociation,
} from "@/shared/lib/projectAgentAssociation";
import { normalizePubkey, truncatePubkey } from "@/shared/lib/pubkey";

import {
  collectProjectAgentEvidence,
  compareSessionsNewestFirst,
  type ProjectAgentAssignment,
  type ProjectAgentEvidence,
  type ProjectAgentSession,
  type ProjectAgentsDeclaredSessionInput,
  type ProjectAgentsExecutionInput,
  type ProjectAgentsUmbrellaInput,
} from "./projectAgentsEvidence";

export type {
  ProjectAgentAssignment,
  ProjectAgentOpenTarget,
  ProjectAgentSession,
  ProjectAgentsDeclaredSessionInput,
  ProjectAgentsExecutionInput,
  ProjectAgentsUmbrellaInput,
} from "./projectAgentsEvidence";

/** The role the setup bootstrap runs as; native refuses to associate it. */
export const PROJECT_SETUP_ROLE = "project-setup";

/** A managed agent record on this computer. */
export type ProjectAgentsLocalAgentInput = {
  pubkey: string;
  name: string;
  avatarUrl?: string | null;
  homeRole: string | null;
  projectRef?: string | null;
};

export type BuildProjectAgentsInput = {
  /** The project's own address, `30621:<owner>:<d>`. */
  projectRef: string;
  executions: readonly ProjectAgentsExecutionInput[];
  umbrellas: readonly ProjectAgentsUmbrellaInput[];
  declaredSessions: readonly ProjectAgentsDeclaredSessionInput[];
  /** `installedRolesForProject(...)` for this project. Not membership. */
  installations: readonly {
    role: string;
    agentPubkey: string;
    packRef: PackRef;
  }[];
  /** Every managed agent record on this computer. */
  localAgents: readonly ProjectAgentsLocalAgentInput[];
  /** Accepted published associations (authorized authors, this digest). */
  publishedAgents: readonly PublishedAgentAssociation[];
  /** Relay identity and profile names for anyone else. */
  otherNames?: ReadonlyMap<string, string>;
  /** Display names of listed projects, keyed by normalized coordinate. */
  projectNames?: ReadonlyMap<string, string>;
  nowSeconds: number;
};

export type ProjectAgentInstallation = {
  role: string;
  packRef: PackRef;
};

/**
 * - `project`: associated with this project (or installed for it and not yet
 *   associated, with `associationMissing`).
 * - `borrowed`: evidence in an open session, not associated.
 * - `previous`: evidence only in closed sessions, not associated.
 */
export type ProjectAgentSection = "project" | "borrowed" | "previous";

/**
 * - `working` — newest open execution is starting, running or waiting for input.
 * - `idle` — newest open execution is idle.
 * - `disconnected` — newest open execution is disconnected or interrupted.
 * - `available` — associated, on this computer, no live execution.
 * - `not-associated` — installed here for the project, not associated, no
 *   live execution: the lead cannot hire it.
 * - `elsewhere` — published association, no record here, no live execution.
 * - `not-running` — borrowed, no live execution.
 * - `historical` — closed sessions only.
 */
export type ProjectAgentState =
  | "working"
  | "idle"
  | "disconnected"
  | "available"
  | "not-associated"
  | "elsewhere"
  | "not-running"
  | "historical";

export type ProjectAgentLocation =
  | { kind: "here" }
  | { kind: "elsewhere"; ownerPubkey: string; ownerName: string }
  | { kind: "unknown" };

/** The newest session or assignment, as facts; the copy writes the sentence. */
export type ProjectAgentRelationship =
  | {
      kind: "seated";
      role: string | null;
      sessionName: string;
      assignerName: string | null;
    }
  | {
      kind: "assigned";
      role: string;
      sessionName: string;
      assignerName: string;
    };

export type ProjectAgentRow = {
  pubkey: string;
  name: string;
  avatarUrl: string | null;
  section: ProjectAgentSection;
  /** Whether this identity belongs to the project, whatever its section. */
  isProjectAgent: boolean;
  state: ProjectAgentState;
  location: ProjectAgentLocation;
  /** The agent's home role (local record, then its publication), or `null`. */
  primaryRole: string | null;
  /** Distinct roles it was seated or assigned as here, sorted. */
  seatedRoles: string[];
  /** A managed record for this pubkey exists on this computer. */
  managedHere: boolean;
  /** Installed for this project here, but the record lacks the association. */
  associationMissing: boolean;
  /** The other project its local record belongs to, when not this one. */
  otherProject: { ref: string; name: string | null } | null;
  /**
   * The record may be associated with this project: here, unassociated, with
   * a primary role that is not the setup bootstrap. Viewer access is decided
   * by the view, and native re-checks everything.
   */
  mayAssociate: boolean;
  relationship: ProjectAgentRelationship | null;
  installations: ProjectAgentInstallation[];
  /** Open sessions first, freshest first. */
  sessions: ProjectAgentSession[];
  /** Newest first. */
  assignments: ProjectAgentAssignment[];
  /** The freshest observed status age across sessions, or `null`. */
  lastSeenSeconds: number | null;
};

export type ProjectAgentsModel = {
  projectAgents: ProjectAgentRow[];
  borrowed: ProjectAgentRow[];
  previous: ProjectAgentRow[];
};

function compareStrings(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

/** The live word an execution status earns, or `null` when it is not live. */
export function liveStateOf(
  status: CodingSessionStatus,
): "working" | "idle" | "disconnected" | null {
  switch (status) {
    case "starting":
    case "running":
    case "waiting_for_input":
      return "working";
    case "idle":
      return "idle";
    case "disconnected":
    case "interrupted":
      return "disconnected";
    default:
      return null;
  }
}

function relationshipOf(
  evidence: ProjectAgentEvidence | undefined,
): ProjectAgentRelationship | null {
  if (!evidence) return null;
  const session =
    evidence.sessions.find((candidate) => !candidate.sessionClosed) ??
    evidence.sessions[0] ??
    null;
  if (session) {
    // Only a signed assignment in that same session names who put it to work:
    // a seat grant's signer is not on this projection, a hire's requester is a claim.
    const assignment =
      session.sessionRef === null
        ? null
        : (evidence.assignments.find(
            (candidate) => candidate.sessionRef === session.sessionRef,
          ) ?? null);
    return {
      kind: "seated",
      role: session.role ?? assignment?.role ?? null,
      sessionName: session.sessionName,
      assignerName: assignment?.assignerName ?? null,
    };
  }
  const assignment =
    evidence.assignments.find((candidate) => !candidate.sessionClosed) ??
    evidence.assignments[0] ??
    null;
  if (!assignment) return null;
  return {
    kind: "assigned",
    role: assignment.role,
    sessionName: assignment.sessionName,
    assignerName: assignment.assignerName,
  };
}

/** Build the three sections of one project's Agents tab. */
export function buildProjectAgents(
  input: BuildProjectAgentsInput,
): ProjectAgentsModel {
  const project = normalizeProjectCoordinate(input.projectRef);
  const model: ProjectAgentsModel = {
    projectAgents: [],
    borrowed: [],
    previous: [],
  };
  if (project === null) return model;
  const digest = projectAgentDigest(project);

  const local = new Map(
    input.localAgents.map(
      (agent) => [normalizePubkey(agent.pubkey), agent] as const,
    ),
  );
  const published = new Map(
    input.publishedAgents
      .filter((association) => association.projectDigest === digest)
      .map((association) => [normalizePubkey(association.pubkey), association]),
  );
  const nameOf = (pubkey: string): string => {
    const key = normalizePubkey(pubkey);
    return (
      local.get(key)?.name ??
      published.get(key)?.name ??
      input.otherNames?.get(key) ??
      truncatePubkey(key)
    );
  };

  const installationsByAgent = new Map<string, ProjectAgentInstallation[]>();
  for (const installation of input.installations) {
    const key = normalizePubkey(installation.agentPubkey);
    const list = installationsByAgent.get(key) ?? [];
    if (!list.some((existing) => existing.role === installation.role)) {
      list.push({ role: installation.role, packRef: installation.packRef });
    }
    installationsByAgent.set(key, list);
  }

  const evidence = collectProjectAgentEvidence({
    projectRef: project,
    executions: input.executions,
    umbrellas: input.umbrellas,
    declaredSessions: input.declaredSessions,
    installedSha: (pubkey, role) =>
      installationsByAgent
        .get(pubkey)
        ?.find((installation) => installation.role === role)?.packRef.sha,
    nameOf,
    nowSeconds: input.nowSeconds,
  });

  const candidates = new Set<string>([...published.keys(), ...evidence.keys()]);
  for (const [pubkey, agent] of local) {
    if (normalizeProjectCoordinate(agent.projectRef) === project) {
      candidates.add(pubkey);
    }
  }
  for (const pubkey of installationsByAgent.keys()) candidates.add(pubkey);

  for (const pubkey of candidates) {
    const record = local.get(pubkey) ?? null;
    const recordProject = normalizeProjectCoordinate(record?.projectRef);
    const claim = record ? null : (published.get(pubkey) ?? null);
    const installations = installationsByAgent.get(pubkey) ?? [];
    const associationMissing =
      record !== null && recordProject === null && installations.length > 0;
    const isProjectAgent =
      recordProject === project || claim !== null || associationMissing;

    const agentEvidence = evidence.get(pubkey);
    const sessions = agentEvidence?.sessions ?? [];
    const assignments = agentEvidence?.assignments ?? [];
    const hasOpen =
      sessions.some((session) => !session.sessionClosed) ||
      assignments.some((assignment) => !assignment.sessionClosed);
    const hasEvidence = sessions.length > 0 || assignments.length > 0;

    let section: ProjectAgentSection;
    if (isProjectAgent) section = "project";
    else if (hasOpen) section = "borrowed";
    else if (hasEvidence) section = "previous";
    // An installation whose record belongs to another project, or is gone,
    // with no evidence here: nothing to list.
    else continue;

    const newestOpen = sessions
      .filter((session) => !session.sessionClosed)
      .sort(compareSessionsNewestFirst)[0];
    const live = newestOpen ? liveStateOf(newestOpen.status) : null;
    let state: ProjectAgentState;
    if (live) state = live;
    else if (section === "project") {
      state = !record
        ? "elsewhere"
        : associationMissing
          ? "not-associated"
          : "available";
    } else if (section === "borrowed") state = "not-running";
    else state = "historical";

    const location: ProjectAgentLocation = record
      ? { kind: "here" }
      : claim
        ? {
            kind: "elsewhere",
            ownerPubkey: normalizePubkey(claim.ownerPubkey),
            ownerName: nameOf(claim.ownerPubkey),
          }
        : { kind: "unknown" };

    const primaryRole =
      record?.homeRole?.trim() || claim?.homeRole?.trim() || null;
    const seatedRoles = new Set<string>();
    for (const session of sessions) {
      if (session.role) seatedRoles.add(session.role);
    }
    for (const assignment of assignments) seatedRoles.add(assignment.role);

    const ages = sessions
      .map((session) => session.ageSeconds)
      .filter((age): age is number => age !== null);

    const row: ProjectAgentRow = {
      pubkey,
      name: nameOf(pubkey),
      avatarUrl: record?.avatarUrl ?? null,
      section,
      isProjectAgent,
      state,
      location,
      primaryRole,
      seatedRoles: [...seatedRoles].sort(compareStrings),
      managedHere: record !== null,
      associationMissing,
      otherProject:
        recordProject !== null && recordProject !== project
          ? {
              ref: recordProject,
              name: input.projectNames?.get(recordProject) ?? null,
            }
          : null,
      mayAssociate:
        record !== null &&
        recordProject === null &&
        primaryRole !== null &&
        primaryRole !== PROJECT_SETUP_ROLE,
      relationship: relationshipOf(agentEvidence),
      installations,
      sessions,
      assignments,
      lastSeenSeconds: ages.length > 0 ? Math.min(...ages) : null,
    };
    if (section === "project") model.projectAgents.push(row);
    else if (section === "borrowed") model.borrowed.push(row);
    else model.previous.push(row);
  }

  const byName = (a: ProjectAgentRow, b: ProjectAgentRow) =>
    a.name.localeCompare(b.name) || compareStrings(a.pubkey, b.pubkey);
  const byFreshness = (a: ProjectAgentRow, b: ProjectAgentRow) => {
    const ageA = a.lastSeenSeconds ?? Number.POSITIVE_INFINITY;
    const ageB = b.lastSeenSeconds ?? Number.POSITIVE_INFINITY;
    return ageA !== ageB ? ageA - ageB : byName(a, b);
  };
  model.projectAgents.sort(byName);
  model.borrowed.sort(byName);
  model.previous.sort(byFreshness);
  return model;
}
