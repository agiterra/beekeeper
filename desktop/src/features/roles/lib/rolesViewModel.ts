/**
 * The Roles tab's pure model: the ladder's packs, the managed agents, the
 * coding-session shelf and the project list, joined into what the tab draws.
 * No React here — every decision about what a row says is a function a node
 * test can call.
 *
 * Join keys: a seat's `session.role` to a pack's `role`, a seat's
 * `session.agentRef` to `agent.pubkey`, a shelf entry's `projectId` to
 * `project.id`. Nothing is inferred across them. A seat whose role has no
 * pack is listed under a row that says so; a seat whose agent is not managed
 * here keeps its pubkey and may use a shared identity name; an agent whose pack is missing carries
 * that fact onto its chip; a seat with no `packRef` has no sha.
 *
 * Inside a project, a role card's agents are that project's agents: local
 * records associated with the project (`ManagedAgent.projectRef`) whose
 * primary role is the card's role. An identity that merely held a session
 * here — a borrowed worker, another computer's agent — is listed apart as
 * `nonProjectAgents` and never counted as the role's agent.
 */
import type { ProjectCodingSessionShelfEntry } from "@/features/projects-container/lib/projectCodingSessionShelf";
import {
  normalizeProjectRef,
  type ProjectContainer,
} from "@/features/projects-container/lib/projectContainerModel";
import type { CodingSessionCatalogRecord } from "@/features/coding-sessions/lib/codingSessionTypes";
import {
  agentProjectRelation,
  sameProjectRef,
} from "@/shared/lib/projectAgentAssociation";
import type {
  ManagedAgent,
  RelayAgent,
  RolePackOrigin,
  RolePackRef,
  RolePackSkill,
  RolePackSummary,
} from "@/shared/api/types";

import type { SeatRow } from "./seatRows";
import { buildSeatRows } from "./seatRows";

// Re-exported so existing imports (`SeatRowButton.tsx`, tests) keep working
// after the seat-row join moved to `seatRows.ts`, which U3's Contributors
// view also builds on.
export type { SeatRow } from "./seatRows";
export { seatAgeSeconds } from "./seatRows";

/** An agent associated with the role by a local definition or scoped session. */
export type RoleAgentChip = {
  pubkey: string;
  name: string;
  /** `ManagedAgent.hasRolePack` as is: `false` is the disclosure, `undefined` was never asked. */
  hasRolePack: boolean | undefined;
  /** `ManagedAgent.packRefusedSharedHome` as is. */
  packRefusedSharedHome: boolean | undefined;
  /** Local process status only; relay presence is not process status. */
  status: ManagedAgent["status"] | undefined;
  /** Identity image when known; null draws initials. */
  avatarUrl: string | null;
  /** Local runtime setting; null for an unknown remote runtime. */
  runtime: string | null;
  /** Local model setting; null for an unknown remote model. */
  model: string | null;
  /** Whether this computer manages this agent record. */
  isManagedHere?: boolean;
  ownerPubkey?: string | null;
  /**
   * Inside a project: `project` for this project's agent on this computer;
   * `not-associated` / `other-project` for a local record that is not;
   * `unconfirmed` for an identity this computer has no record of (its
   * association is published, and the Agents tab reads it). Absent outside
   * a project.
   */
  projectAgent?: "project" | "not-associated" | "other-project" | "unconfirmed";
};

/** One role card. */
export type RoleRow = {
  role: string;
  /**
   * `true` when the ladder produced a pack for this role. A `false` row
   * exists only because a seat or an agent names the role; its pack fields
   * are empty, never filled in from the slug.
   */
  hasPack: boolean;
  displayName: string;
  description: string;
  summary: string;
  version: string | null;
  origin: RolePackOrigin | null;
  packDir: string | null;
  packRef: RolePackRef | null;
  skills: RolePackSkill[];
  refusal: string | null;
  /** Inside a project: this project's agents whose primary role this is. */
  agents: RoleAgentChip[];
  /**
   * Inside a project: identities that held a session in this role here but
   * are not this project's agents on this computer. Never counted as agents.
   */
  nonProjectAgents?: RoleAgentChip[];
  seats: SeatRow[];
};

/** One project block in Section B. */
export type ProjectAgentsRow = {
  projectId: string;
  projectName: string;
  seats: SeatRow[];
};

export type RolesView = {
  /** Sorted by slug. */
  roles: RoleRow[];
  /** In the projects' display order; a project with no seats is still here. */
  byProject: ProjectAgentsRow[];
  /** Seats no listed project claims. */
  unplaced: SeatRow[];
};

export type BuildRolesViewInput = {
  rolePacks: readonly RolePackSummary[];
  agents: readonly ManagedAgent[];
  /** `useProjectCodingSessionBuckets`: `byProject` flattened plus `unclaimed`. */
  shelfEntries: readonly ProjectCodingSessionShelfEntry[];
  projects: readonly ProjectContainer[];
  nowSeconds: number;
  /** When present, project boundaries apply to every row and participant. */
  projectId?: string;
  relayAgents?: readonly RelayAgent[];
  /**
   * Raw execution records before the umbrella fold. A worker seated inside
   * another agent's session is an execution of that umbrella, not a shelf row,
   * so without these it never counts as a participant.
   */
  executions?: readonly {
    session: Pick<
      CodingSessionCatalogRecord,
      "agentRef" | "role" | "projectRef"
    >;
  }[];
};

function homeRoleOf(agent: ManagedAgent): string | null {
  const role = agent.homeRole?.trim();
  return role ? role : null;
}

function compareStrings(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

function agentChip(
  agent: ManagedAgent,
  ownerPubkey?: string | null,
): RoleAgentChip {
  return {
    pubkey: agent.pubkey.toLowerCase(),
    name: agent.name,
    hasRolePack: agent.hasRolePack,
    packRefusedSharedHome: agent.packRefusedSharedHome,
    status: agent.status,
    avatarUrl: agent.avatarUrl,
    runtime: agent.runtime,
    model: agent.model,
    isManagedHere: true,
    ownerPubkey,
  };
}

function compareChips(a: RoleAgentChip, b: RoleAgentChip): number {
  return compareStrings(a.name, b.name) || compareStrings(a.pubkey, b.pubkey);
}

function roleRow(
  slug: string,
  pack: RolePackSummary | null,
  agents: RoleAgentChip[],
  nonProjectAgents: RoleAgentChip[],
  seats: SeatRow[],
): RoleRow {
  if (!pack) {
    return {
      role: slug,
      hasPack: false,
      displayName: slug,
      description: "",
      summary: "",
      version: null,
      origin: null,
      packDir: null,
      packRef: null,
      skills: [],
      refusal: null,
      agents,
      nonProjectAgents,
      seats,
    };
  }
  return {
    role: slug,
    hasPack: true,
    displayName: pack.displayName,
    description: pack.description,
    summary: pack.summary,
    version: pack.version,
    origin: pack.origin,
    packDir: pack.packDir,
    packRef: pack.packRef,
    skills: pack.skills,
    refusal: pack.refusal,
    agents,
    nonProjectAgents,
    seats,
  };
}

type ExecutionInput = NonNullable<BuildRolesViewInput["executions"]>[number];

type ProjectRoleAgents = {
  agents: Map<string, RoleAgentChip[]>;
  nonProjectAgents: Map<string, RoleAgentChip[]>;
};

function pushChip(
  byRole: Map<string, RoleAgentChip[]>,
  role: string,
  chip: RoleAgentChip,
) {
  const chips = byRole.get(role) ?? [];
  if (chips.some((existing) => existing.pubkey === chip.pubkey)) return;
  chips.push(chip);
  byRole.set(role, chips);
}

/**
 * A project's role agents are its associated local agents, by primary role.
 * Everyone else who held a session in the project (shelf rows, and workers
 * seated inside another agent's umbrella) is listed apart under the session's
 * role: seen here, not this project's agent. Channel membership, a matching
 * role name or an installed pack never makes an agent a project agent.
 */
function projectRoleAgents(
  agents: readonly ManagedAgent[],
  relayAgents: readonly RelayAgent[],
  entries: readonly ProjectCodingSessionShelfEntry[],
  executions: readonly ExecutionInput[],
  projectAddress: string | null,
): ProjectRoleAgents {
  const relayByKey = new Map(
    relayAgents.map((agent) => [agent.pubkey.toLowerCase(), agent]),
  );
  const localByKey = new Map(
    agents.map((agent) => [agent.pubkey.toLowerCase(), agent]),
  );
  const result: ProjectRoleAgents = {
    agents: new Map(),
    nonProjectAgents: new Map(),
  };
  const projectKeys = new Set<string>();
  for (const agent of agents) {
    const role = homeRoleOf(agent);
    if (!role || !sameProjectRef(agent.projectRef, projectAddress)) continue;
    const key = agent.pubkey.toLowerCase();
    projectKeys.add(key);
    pushChip(result.agents, role, {
      ...agentChip(agent, relayByKey.get(key)?.ownerPubkey),
      projectAgent: "project",
    });
  }
  const sightings: { agentRef: string | null | undefined; role: unknown }[] =
    entries.map((entry) => entry.session);
  const wantedRef = projectAddress
    ? (normalizeProjectRef(projectAddress) ?? projectAddress)
    : null;
  for (const { session } of executions) {
    if (!wantedRef || !session.projectRef) continue;
    if (
      (normalizeProjectRef(session.projectRef) ?? session.projectRef) !==
      wantedRef
    ) {
      continue;
    }
    sightings.push(session);
  }
  // Closed sessions still count as having been here; buildSeatRows below
  // keeps them out of the open seats.
  for (const sighting of sightings) {
    const key = sighting.agentRef?.toLowerCase();
    const role = typeof sighting.role === "string" ? sighting.role.trim() : "";
    if (!key || !role || projectKeys.has(key)) continue;
    const local = localByKey.get(key);
    const relay = relayByKey.get(key);
    if (!local && !relay) continue; // Unknown identities remain on session rows.
    const chip: RoleAgentChip = local
      ? {
          ...agentChip(local, relay?.ownerPubkey),
          projectAgent:
            agentProjectRelation(local, projectAddress) === "other-project"
              ? "other-project"
              : "not-associated",
        }
      : {
          pubkey: key,
          name: relay?.name ?? key,
          hasRolePack: undefined,
          packRefusedSharedHome: undefined,
          status: undefined,
          avatarUrl: null,
          runtime: null,
          model: null,
          isManagedHere: false,
          ownerPubkey: relay?.ownerPubkey,
          projectAgent: "unconfirmed",
        };
    pushChip(result.nonProjectAgents, role, chip);
  }
  return result;
}

/** Join packs, agents, shelf and projects into the three lists the tab draws. */
export function buildRolesView(input: BuildRolesViewInput): RolesView {
  const { rolePacks, agents, nowSeconds, projectId, relayAgents = [] } = input;
  const scoped = projectId !== undefined;
  const shelfEntries = scoped
    ? input.shelfEntries.filter((entry) => entry.projectId === projectId)
    : input.shelfEntries;
  const projects = scoped
    ? input.projects.filter((project) => project.id === projectId)
    : input.projects;
  const projectsById = new Map(
    projects.map((project) => [project.id, project] as const),
  );
  const seats = buildSeatRows({
    shelfEntries,
    agents,
    relayAgents,
    projects,
    nowSeconds,
  });
  const scopedAgents = scoped
    ? projectRoleAgents(
        agents,
        relayAgents,
        shelfEntries,
        input.executions ?? [],
        projects[0]?.address ?? null,
      )
    : null;
  const agentsByRole =
    scopedAgents?.agents ?? new Map<string, RoleAgentChip[]>();
  const nonProjectByRole =
    scopedAgents?.nonProjectAgents ?? new Map<string, RoleAgentChip[]>();
  if (!scoped) {
    for (const agent of agents) {
      const role = homeRoleOf(agent);
      if (!role) continue;
      const chips = agentsByRole.get(role) ?? [];
      chips.push(agentChip(agent));
      agentsByRole.set(role, chips);
    }
  }
  const packsByRole = new Map(rolePacks.map((pack) => [pack.role, pack]));
  const slugs = new Set([
    ...packsByRole.keys(),
    ...agentsByRole.keys(),
    ...nonProjectByRole.keys(),
  ]);
  for (const seat of seats) {
    if (seat.role) slugs.add(seat.role);
  }
  const roles = [...slugs].sort(compareStrings).map((slug) =>
    roleRow(
      slug,
      packsByRole.get(slug) ?? null,
      (agentsByRole.get(slug) ?? []).sort(compareChips),
      (nonProjectByRole.get(slug) ?? []).sort(compareChips),
      seats.filter((seat) => seat.role === slug),
    ),
  );
  const byProject = projects.map((project) => ({
    projectId: project.id,
    projectName: project.name,
    seats: seats.filter((seat) => seat.projectId === project.id),
  }));
  const unplaced = scoped
    ? []
    : seats.filter(
        (seat) => seat.projectId === null || !projectsById.has(seat.projectId),
      );
  return { roles, byProject, unplaced };
}

/** What the header sentence can say about a project's packs as a whole. */
export type PacksSourceSummary = {
  /** The distinct origins, sorted; one entry when every role shares it. */
  origins: RolePackOrigin[];
  /**
   * The one `packRef.repo` every role shares, else the one parent directory
   * every `packDir` shares, else `null` — the sentence then says "several".
   */
  location: string | null;
  /** The one sha every role shares; `null` when any is unknown or they differ. */
  sha: string | null;
  /** `true` when two roles carry different known shas. */
  shasDiffer: boolean;
};

function parentDir(dir: string): string {
  const cut = Math.max(dir.lastIndexOf("/"), dir.lastIndexOf("\\"));
  return cut > 0 ? dir.slice(0, cut) : dir;
}

/**
 * Summarize the packs for the header. A value is reported only when every
 * role agrees on it; the sentence would otherwise be claiming one role's
 * origin or sha for all of them.
 */
export function describePacksSource(
  packs: readonly RolePackSummary[],
): PacksSourceSummary {
  const origins = [...new Set(packs.map((pack) => pack.origin))].sort(
    compareStrings,
  );
  if (packs.length === 0) {
    return { origins, location: null, sha: null, shasDiffer: false };
  }
  const refs = packs.map((pack) => pack.packRef);
  const firstRepo = refs[0]?.repo ?? null;
  const firstParent = parentDir(packs[0]?.packDir ?? "");
  let location: string | null = null;
  if (
    firstRepo !== null &&
    refs.every((ref) => ref !== null && ref.repo === firstRepo)
  ) {
    location = firstRepo;
  } else if (packs.every((pack) => parentDir(pack.packDir) === firstParent)) {
    location = firstParent;
  }
  const knownShas = new Set(
    refs.flatMap((ref) => (ref === null ? [] : [ref.sha])),
  );
  const firstSha = refs[0]?.sha ?? null;
  const sha =
    firstSha !== null &&
    refs.every((ref) => ref !== null && ref.sha === firstSha)
      ? firstSha
      : null;
  return { origins, location, sha, shasDiffer: knownShas.size > 1 };
}

/** Local pack availability; remote availability is not observed by this model. */
export type RolePackChipState =
  | "refused"
  | "missing"
  | "blocked"
  | "present"
  | "unknown";

/**
 * A remote agent's pack availability is unknown: this computer's resolved
 * roles and refusals say nothing about another host. For local agents, use
 * the role row rather than an optional per-agent availability probe, with
 * the agent's shared-home refusal taking precedence over the role result.
 * "present" means available here, never proof that instructions were staged
 * or executed. Omitted isManagedHere preserves the existing local-only callers.
 */
export function roleAgentPackState(input: {
  roleHasPack: boolean;
  roleRefusal: string | null;
  agentPackRefusedSharedHome: boolean | undefined;
  isManagedHere?: boolean;
}): RolePackChipState {
  if (input.isManagedHere === false) return "unknown";
  if (input.agentPackRefusedSharedHome === true) return "refused";
  if (input.roleHasPack === false) return "missing";
  if (input.roleRefusal !== null) return "blocked";
  return "present";
}
