/**
 * What a project's signed records say each identity did there: executions
 * (kind 44223, read **before** the umbrella fold so a worker seated inside a
 * lead's session is visible) and assignments (kind 44244, the declared-work
 * projection).
 *
 * Evidence is history and state, never membership: an identity's section on
 * the Agents tab is decided by association (`projectAgentsModel.ts`). The
 * umbrella shelf is used only to name a session, say whether it is closed,
 * and find where to open it.
 */
import { buildCodingSessionExecutionKey } from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import type { PackRef } from "@/features/coding-sessions/lib/codingSessionPackRef";
import type {
  CodingSessionCatalogRecord,
  CodingSessionStatus,
} from "@/features/coding-sessions/lib/codingSessionTypes";
import type { ProjectCodingSessionShelfEntry } from "@/features/projects-container/lib/projectCodingSessionShelf";
import { seatAgeSeconds } from "@/features/roles/lib/seatRows";
import type {
  PulseDeclaredAssignmentStatus,
  PulseDeclaredDispositionDecision,
  PulseDeclaredWorkSession,
} from "@/features/project-pulse/lib/pulseDeclaredWorkWire";
import { sameProjectRef } from "@/shared/lib/projectAgentAssociation";
import { normalizePubkey } from "@/shared/lib/pubkey";

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

/** Where a row opens: a started generation, or a founded umbrella. */
export type ProjectAgentOpenTarget = {
  channelId: string;
  generationId: string;
  founded: boolean;
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

export type ProjectAgentEvidence = {
  pubkey: string;
  sessions: ProjectAgentSession[];
  assignments: ProjectAgentAssignment[];
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
  return `${entry.channelId}|${target}`;
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

/** Freshest observed status first; unobserved last; stable by key. */
export function compareSessionsNewestFirst(
  a: ProjectAgentSession,
  b: ProjectAgentSession,
): number {
  const ageA = a.ageSeconds ?? Number.POSITIVE_INFINITY;
  const ageB = b.ageSeconds ?? Number.POSITIVE_INFINITY;
  if (ageA !== ageB) return ageA - ageB;
  return compareStrings(a.key, b.key);
}

/** Open sessions first, then freshest first. */
function compareSessionsForDisplay(
  a: ProjectAgentSession,
  b: ProjectAgentSession,
): number {
  if (a.sessionClosed !== b.sessionClosed) return a.sessionClosed ? 1 : -1;
  return compareSessionsNewestFirst(a, b);
}

/**
 * Collect every identity's executions and assignments in one project.
 *
 * `installedSha(pubkey, role)` answers the revision this computer installed
 * for that agent and role, so a staged revision that differs can be flagged.
 */
export function collectProjectAgentEvidence(input: {
  projectRef: string;
  executions: readonly ProjectAgentsExecutionInput[];
  umbrellas: readonly ProjectAgentsUmbrellaInput[];
  declaredSessions: readonly ProjectAgentsDeclaredSessionInput[];
  installedSha: (pubkey: string, role: string) => string | undefined;
  nameOf: (pubkey: string) => string;
  nowSeconds: number;
}): Map<string, ProjectAgentEvidence> {
  const umbrellaByRef = new Map<string, ProjectAgentsUmbrellaInput>();
  for (const umbrella of input.umbrellas) {
    if (umbrella.sessionRef) umbrellaByRef.set(umbrella.sessionRef, umbrella);
  }

  const evidence = new Map<string, ProjectAgentEvidence>();
  const evidenceFor = (pubkey: string): ProjectAgentEvidence => {
    const key = normalizePubkey(pubkey);
    let entry = evidence.get(key);
    if (!entry) {
      entry = { pubkey: key, sessions: [], assignments: [] };
      evidence.set(key, entry);
    }
    return entry;
  };

  for (const entry of activeExecutions(input.executions)) {
    const { session } = entry;
    if (!session.agentRef) continue;
    if (!sameProjectRef(session.projectRef, input.projectRef)) continue;
    const acc = evidenceFor(session.agentRef);
    const umbrella = session.sessionRef
      ? (umbrellaByRef.get(session.sessionRef) ?? null)
      : null;
    const role = session.role?.trim() || null;
    const installedSha = role
      ? input.installedSha(acc.pubkey, role)
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
      const acc = evidenceFor(assignment.assigneeActor);
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
        assignerName: input.nameOf(assignment.assignerPubkey),
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

  for (const entry of evidence.values()) {
    entry.sessions.sort(compareSessionsForDisplay);
    entry.assignments.sort(
      (a, b) => b.createdAt - a.createdAt || compareStrings(a.key, b.key),
    );
  }
  return evidence;
}
