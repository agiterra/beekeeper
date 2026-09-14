/**
 * The project Agents tab — who is working in one project, and why.
 *
 * One row per agent identity, built only from three records
 * (`docs/PROJECT_AGENTS_TAB_SPEC.md` § Membership rule):
 *
 * 1. this computer's setup journal installed it for the project;
 * 2. a signed execution (kind 44223) placed in the project names it as
 *    `agentRef`;
 * 3. a canonically included assignment (kind 44244) in one of the project's
 *    sessions names it as `assigneeActor`.
 *
 * A matching home role, channel membership or a name never lists an agent.
 *
 * Executions are read **before** the umbrella fold: a worker seated inside a
 * lead's session is an execution of that umbrella, not a shelf row of its own,
 * and reading the shelf alone is exactly how Bob came to be invisible beside
 * his own report. The umbrella shelf is used only to name a session, say
 * whether it is closed, and find where to open it.
 */
import { buildCodingSessionExecutionKey } from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import type { PackRef } from "@/features/coding-sessions/lib/codingSessionPackRef";
import type {
  CodingSessionCatalogRecord,
  CodingSessionStatus,
} from "@/features/coding-sessions/lib/codingSessionTypes";
import type { ProjectCodingSessionShelfEntry } from "@/features/projects-container/lib/projectCodingSessionShelf";
import { normalizeProjectRef } from "@/features/projects-container/lib/projectContainerModel";
import { seatAgeSeconds, seatIsLive } from "@/features/roles/lib/seatRows";
import type {
  PulseDeclaredAssignmentStatus,
  PulseDeclaredDispositionDecision,
  PulseDeclaredWorkSession,
} from "@/features/project-pulse/lib/pulseDeclaredWorkWire";
import { normalizePubkey, truncatePubkey } from "@/shared/lib/pubkey";

/** One raw execution generation, as the global catalog read it. */
export type ProjectAgentsExecutionInput = {
  channelId: string;
  session: Pick<
    CodingSessionCatalogRecord,
    | "generationId"
    | "label"
    | "agentRef"
    | "role"
    | "projectRef"
    | "sessionRef"
    | "provider"
    | "runtime"
    | "model"
    | "status"
    | "statusAt"
    | "packRef"
    | "providerAuthorityPubkey"
    | "commandTarget"
  >;
};

/** One umbrella row, used to name, close and open a session. */
export type ProjectAgentsUmbrellaInput = Pick<
  ProjectCodingSessionShelfEntry,
  "channelId" | "generationId" | "label" | "sessionRef" | "isClosed" | "founded"
>;

/** One umbrella's declared work, as the native projection wrote it. */
export type ProjectAgentsDeclaredSessionInput = Pick<
  PulseDeclaredWorkSession,
  "sessionRef" | "channelId" | "name" | "lifecycle" | "assignments"
>;

/** A name this build knows for a pubkey: managed record, relay identity or profile. */
export type ProjectAgentsIdentityInput = {
  pubkey: string;
  name: string | null;
  avatarUrl?: string | null;
  /** A managed agent record for this pubkey exists on this computer. */
  managedHere: boolean;
};

export type BuildProjectAgentsInput = {
  /** The project's own address, `30621:<owner>:<d>`. */
  projectRef: string;
  executions: readonly ProjectAgentsExecutionInput[];
  umbrellas: readonly ProjectAgentsUmbrellaInput[];
  declaredSessions: readonly ProjectAgentsDeclaredSessionInput[];
  /** `installedRolesForProject(...)` for this project. */
  installations: readonly {
    role: string;
    agentPubkey: string;
    packRef: PackRef;
  }[];
  /** Agents (managed or relay) — the only identities that may carry a name here. */
  agents: readonly ProjectAgentsIdentityInput[];
  /** People and other signers that may author an assignment; names only. */
  otherNames?: ReadonlyMap<string, string>;
  nowSeconds: number;
};

/** Where a row opens: a started generation, or a founded umbrella. */
export type ProjectAgentOpenTarget = {
  channelId: string;
  generationId: string;
  founded: boolean;
};

export type ProjectAgentInstallation = {
  role: string;
  packRef: PackRef;
};

export type ProjectAgentSession = {
  key: string;
  sessionRef: string | null;
  sessionName: string;
  sessionClosed: boolean;
  openTarget: ProjectAgentOpenTarget;
  role: string | null;
  provider: string | null;
  runtime: string | null;
  model: string | null;
  status: CodingSessionStatus;
  ageSeconds: number | null;
  packRef: PackRef | null;
  /**
   * True when this execution staged a different revision of the same role's
   * instructions than this computer installed. `false` when either side is
   * unknown — an unknown is not a difference.
   */
  packDiffersFromInstalled: boolean;
  /** The fact-stream signer — the only machine identity the wire carries. */
  providerAuthorityPubkey: string | null;
};

export type ProjectAgentAssignment = {
  key: string;
  sessionRef: string;
  sessionName: string;
  sessionClosed: boolean;
  openTarget: ProjectAgentOpenTarget | null;
  role: string;
  assignerPubkey: string;
  assignerName: string;
  objective: string;
  brief: string;
  acceptanceSteps: readonly string[];
  status: PulseDeclaredAssignmentStatus;
  reportCount: number;
  latestDecision: PulseDeclaredDispositionDecision | null;
  createdAt: number;
};

/**
 * - `working`: appears in at least one session that is not closed. Idle and
 *   stopped executions stay here while their session is open.
 * - `installed`: installed for the project, and no session or assignment names it.
 * - `previous`: every session it appeared in is closed.
 */
export type ProjectAgentSection = "working" | "installed" | "previous";

/** The one sentence a row leads with, as facts; `projectAgentsCopy` writes it. */
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
    }
  | { kind: "installed"; role: string };

export type ProjectAgentRow = {
  pubkey: string;
  name: string;
  avatarUrl: string | null;
  managedHere: boolean;
  section: ProjectAgentSection;
  relationship: ProjectAgentRelationship;
  /** Distinct role slugs this agent holds in the project, sorted. */
  roles: string[];
  installations: ProjectAgentInstallation[];
  /** Open sessions first, live before quiet, freshest first. */
  sessions: ProjectAgentSession[];
  /** Newest first. */
  assignments: ProjectAgentAssignment[];
  /** The freshest observed status age across sessions, or `null`. */
  lastSeenSeconds: number | null;
};

export type ProjectAgentsModel = {
  working: ProjectAgentRow[];
  installed: ProjectAgentRow[];
  previous: ProjectAgentRow[];
};

function compareStrings(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

function executionKey(entry: ProjectAgentsExecutionInput): string {
  const { session } = entry;
  const target = session.commandTarget
    ? buildCodingSessionExecutionKey(
        session.providerAuthorityPubkey,
        session.commandTarget,
      )
    : `untargeted:${session.generationId}`;
  return `${entry.channelId}\u0000${target}`;
}

/** Highest generation per execution; the rest are history of the same seat. */
function activeExecutions(
  executions: readonly ProjectAgentsExecutionInput[],
): ProjectAgentsExecutionInput[] {
  const byKey = new Map<string, ProjectAgentsExecutionInput>();
  for (const entry of executions) {
    const key = executionKey(entry);
    const existing = byKey.get(key);
    if (!existing) {
      byKey.set(key, entry);
      continue;
    }
    const current = existing.session.commandTarget?.generation ?? 0;
    const next = entry.session.commandTarget?.generation ?? 0;
    if (
      next > current ||
      (next === current &&
        compareStrings(
          entry.session.generationId,
          existing.session.generationId,
        ) > 0)
    ) {
      byKey.set(key, entry);
    }
  }
  return [...byKey.values()];
}

function projectKey(ref: string): string {
  return normalizeProjectRef(ref) ?? ref.trim().toLowerCase();
}

function sameProject(ref: string | null, wanted: string): boolean {
  if (!ref) return false;
  return projectKey(ref) === wanted;
}

type Accumulator = {
  pubkey: string;
  installations: ProjectAgentInstallation[];
  sessions: ProjectAgentSession[];
  assignments: ProjectAgentAssignment[];
};

function compareSessions(
  a: ProjectAgentSession,
  b: ProjectAgentSession,
): number {
  if (a.sessionClosed !== b.sessionClosed) return a.sessionClosed ? 1 : -1;
  const liveA = seatIsLive(a.status);
  const liveB = seatIsLive(b.status);
  if (liveA !== liveB) return liveA ? -1 : 1;
  const ageA = a.ageSeconds ?? Number.POSITIVE_INFINITY;
  const ageB = b.ageSeconds ?? Number.POSITIVE_INFINITY;
  if (ageA !== ageB) return ageA - ageB;
  return compareStrings(a.key, b.key);
}

function relationshipOf(acc: Accumulator): ProjectAgentRelationship {
  const session =
    acc.sessions.find((candidate) => !candidate.sessionClosed) ??
    acc.sessions[0] ??
    null;
  if (session) {
    // The newest assignment addressed to this agent in that same session
    // names who put it to work. Nothing else may: a seat grant's signer is not
    // on the projection this page reads, and a hire's requester is a claim.
    const assignment =
      session.sessionRef === null
        ? null
        : (acc.assignments.find(
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
    acc.assignments.find((candidate) => !candidate.sessionClosed) ??
    acc.assignments[0] ??
    null;
  if (assignment) {
    return {
      kind: "assigned",
      role: assignment.role,
      sessionName: assignment.sessionName,
      assignerName: assignment.assignerName,
    };
  }
  // Only reachable with an installation: every accumulator is created by one
  // of the three records.
  return { kind: "installed", role: acc.installations[0]?.role ?? "" };
}

/** Build the three sections of one project's Agents tab. */
export function buildProjectAgents(
  input: BuildProjectAgentsInput,
): ProjectAgentsModel {
  const wantedRef = projectKey(input.projectRef);
  const identities = new Map(
    input.agents.map(
      (agent) => [normalizePubkey(agent.pubkey), agent] as const,
    ),
  );
  const nameOf = (pubkey: string): string => {
    const key = normalizePubkey(pubkey);
    return (
      identities.get(key)?.name ??
      input.otherNames?.get(key) ??
      truncatePubkey(key)
    );
  };

  const umbrellaByRef = new Map<string, ProjectAgentsUmbrellaInput>();
  for (const umbrella of input.umbrellas) {
    if (umbrella.sessionRef) umbrellaByRef.set(umbrella.sessionRef, umbrella);
  }

  const accumulators = new Map<string, Accumulator>();
  const accumulatorFor = (pubkey: string): Accumulator => {
    const key = normalizePubkey(pubkey);
    let acc = accumulators.get(key);
    if (!acc) {
      acc = { pubkey: key, installations: [], sessions: [], assignments: [] };
      accumulators.set(key, acc);
    }
    return acc;
  };

  const installedShaByAgentRole = new Map<string, string>();
  for (const installation of input.installations) {
    const acc = accumulatorFor(installation.agentPubkey);
    if (
      acc.installations.some((existing) => existing.role === installation.role)
    ) {
      continue;
    }
    acc.installations.push({
      role: installation.role,
      packRef: installation.packRef,
    });
    installedShaByAgentRole.set(
      `${acc.pubkey}\u0000${installation.role}`,
      installation.packRef.sha,
    );
  }

  for (const entry of activeExecutions(input.executions)) {
    const { session } = entry;
    if (!session.agentRef) continue;
    if (!sameProject(session.projectRef, wantedRef)) continue;
    const acc = accumulatorFor(session.agentRef);
    const umbrella = session.sessionRef
      ? (umbrellaByRef.get(session.sessionRef) ?? null)
      : null;
    const role = session.role?.trim() || null;
    const installedSha = role
      ? installedShaByAgentRole.get(`${acc.pubkey}\u0000${role}`)
      : undefined;
    acc.sessions.push({
      key: `${entry.channelId}/${session.generationId}`,
      sessionRef: session.sessionRef,
      sessionName: umbrella?.label ?? session.label,
      sessionClosed: umbrella?.isClosed ?? false,
      openTarget: umbrella
        ? {
            channelId: umbrella.channelId,
            generationId: umbrella.generationId,
            founded: umbrella.founded === true,
          }
        : {
            channelId: entry.channelId,
            generationId: session.generationId,
            founded: false,
          },
      role,
      provider: session.provider,
      runtime: session.runtime,
      model: session.model,
      status: session.status,
      ageSeconds: seatAgeSeconds(session.statusAt, input.nowSeconds),
      packRef: session.packRef,
      packDiffersFromInstalled:
        installedSha !== undefined &&
        session.packRef !== null &&
        session.packRef.role === role &&
        session.packRef.sha !== installedSha,
      providerAuthorityPubkey: session.providerAuthorityPubkey,
    });
  }

  for (const declared of input.declaredSessions) {
    const umbrella = umbrellaByRef.get(declared.sessionRef) ?? null;
    const sessionClosed =
      declared.lifecycle === "closed" || umbrella?.isClosed === true;
    const sessionName = umbrella?.label ?? declared.name ?? "Unnamed session";
    for (const assignment of declared.assignments) {
      const acc = accumulatorFor(assignment.assigneeActor);
      if (
        acc.assignments.some(
          (existing) => existing.key === assignment.sourceEventId,
        )
      ) {
        continue;
      }
      const newestDisposition =
        [...assignment.dispositions].sort(
          (a, b) => b.createdAt - a.createdAt,
        )[0] ?? null;
      acc.assignments.push({
        key: assignment.sourceEventId,
        sessionRef: declared.sessionRef,
        sessionName,
        sessionClosed,
        openTarget: umbrella
          ? {
              channelId: umbrella.channelId,
              generationId: umbrella.generationId,
              founded: umbrella.founded === true,
            }
          : null,
        role: assignment.assigneeRole,
        assignerPubkey: normalizePubkey(assignment.assignerPubkey),
        assignerName: nameOf(assignment.assignerPubkey),
        objective: assignment.objective,
        brief: assignment.brief,
        acceptanceSteps: assignment.acceptanceSteps,
        status: assignment.status,
        reportCount: assignment.reports.length,
        latestDecision: newestDisposition?.decision ?? null,
        createdAt: assignment.createdAt,
      });
    }
  }

  const model: ProjectAgentsModel = {
    working: [],
    installed: [],
    previous: [],
  };
  for (const acc of accumulators.values()) {
    acc.sessions.sort(compareSessions);
    acc.assignments.sort(
      (a, b) => b.createdAt - a.createdAt || compareStrings(a.key, b.key),
    );
    const hasOpen =
      acc.sessions.some((session) => !session.sessionClosed) ||
      acc.assignments.some((assignment) => !assignment.sessionClosed);
    const hasAny = acc.sessions.length > 0 || acc.assignments.length > 0;
    const section: ProjectAgentSection = hasOpen
      ? "working"
      : hasAny
        ? "previous"
        : "installed";

    const roles = new Set<string>();
    for (const installation of acc.installations) roles.add(installation.role);
    for (const session of acc.sessions)
      if (session.role) roles.add(session.role);
    for (const assignment of acc.assignments) roles.add(assignment.role);

    const ages = acc.sessions
      .map((session) => session.ageSeconds)
      .filter((age): age is number => age !== null);
    const identity = identities.get(acc.pubkey) ?? null;
    const row: ProjectAgentRow = {
      pubkey: acc.pubkey,
      name: nameOf(acc.pubkey),
      avatarUrl: identity?.avatarUrl ?? null,
      managedHere: identity?.managedHere ?? false,
      section,
      relationship: relationshipOf(acc),
      roles: [...roles].sort(compareStrings),
      installations: acc.installations,
      sessions: acc.sessions,
      assignments: acc.assignments,
      lastSeenSeconds: ages.length > 0 ? Math.min(...ages) : null,
    };
    model[section].push(row);
  }

  const byFreshness = (a: ProjectAgentRow, b: ProjectAgentRow) => {
    const ageA = a.lastSeenSeconds ?? Number.POSITIVE_INFINITY;
    const ageB = b.lastSeenSeconds ?? Number.POSITIVE_INFINITY;
    if (ageA !== ageB) return ageA - ageB;
    return a.name.localeCompare(b.name) || compareStrings(a.pubkey, b.pubkey);
  };
  const byName = (a: ProjectAgentRow, b: ProjectAgentRow) =>
    a.name.localeCompare(b.name) || compareStrings(a.pubkey, b.pubkey);
  model.working.sort(byName);
  model.installed.sort(byName);
  model.previous.sort(byFreshness);
  return model;
}
