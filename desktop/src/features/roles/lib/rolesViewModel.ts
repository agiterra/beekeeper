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
 * here keeps its pubkey and no name; an agent whose pack is missing carries
 * that fact onto its chip; a seat with no `packRef` has no sha.
 */
import type { CodingSessionStatus } from "@/features/coding-sessions/lib/codingSessionTypes";
import type { ProjectCodingSessionShelfEntry } from "@/features/projects-container/lib/projectCodingSessionShelf";
import type { ProjectContainer } from "@/features/projects-container/lib/projectContainerModel";
import type {
  ManagedAgent,
  RolePackOrigin,
  RolePackRef,
  RolePackSkill,
  RolePackSummary,
} from "@/shared/api/types";

/** One seat — a session generation — as the view reads it. */
export type SeatRow = {
  /** `<channelId>/<generationId>`, unique per shelf row. */
  key: string;
  channelId: string;
  generationId: string;
  /** The session's own label, for the row tooltip. */
  label: string;
  /** Lowercased `session.agentRef`, or `null` for a person's session. */
  agentPubkey: string | null;
  /** The managed agent's name, or `null` when no managed agent has that pubkey. */
  agentName: string | null;
  /** The seat's role slug, or `null` when the session carries none. */
  role: string | null;
  projectId: string | null;
  /** The project's name, or `null` when unplaced or the project is not in the list. */
  projectName: string | null;
  /** The catalog record's status word, verbatim. */
  status: CodingSessionStatus;
  /** Seconds since the catalog's `statusAt`, or `null` when no status was observed. */
  ageSeconds: number | null;
  /** The staged pack's sha, or `null` when this generation carried no `packRef`. */
  packSha: string | null;
};

/** A managed agent whose home role is the row's role. */
export type RoleAgentChip = {
  pubkey: string;
  name: string;
  /** `ManagedAgent.hasRolePack` as is: `false` is the disclosure, `undefined` was never asked. */
  hasRolePack: boolean | undefined;
  /** `ManagedAgent.packRefusedSharedHome` as is. */
  packRefusedSharedHome: boolean | undefined;
  status: ManagedAgent["status"];
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
  agents: RoleAgentChip[];
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
};

/**
 * Seconds since a catalog `statusAt`, which is in milliseconds (the newest
 * 44223's `created_at`). `null` stays `null`; a clock that runs ahead of
 * the relay clamps to zero rather than reporting a negative age.
 */
export function seatAgeSeconds(
  statusAtMs: number | null | undefined,
  nowSeconds: number,
): number | null {
  if (
    statusAtMs === null ||
    statusAtMs === undefined ||
    !Number.isFinite(statusAtMs)
  ) {
    return null;
  }
  return Math.max(0, nowSeconds - Math.floor(statusAtMs / 1_000));
}

function homeRoleOf(agent: ManagedAgent): string | null {
  const role = agent.homeRole?.trim();
  return role ? role : null;
}

function seatRow(
  entry: ProjectCodingSessionShelfEntry,
  agentsByPubkey: ReadonlyMap<string, ManagedAgent>,
  projectsById: ReadonlyMap<string, ProjectContainer>,
  nowSeconds: number,
): SeatRow {
  const session = entry.session;
  const agentPubkey = session.agentRef ? session.agentRef.toLowerCase() : null;
  const agent = agentPubkey ? (agentsByPubkey.get(agentPubkey) ?? null) : null;
  const project = entry.projectId
    ? (projectsById.get(entry.projectId) ?? null)
    : null;
  const role = session.role?.trim();
  return {
    key: `${entry.channelId}/${entry.generationId}`,
    channelId: entry.channelId,
    generationId: entry.generationId,
    label: entry.label,
    agentPubkey,
    agentName: agent?.name ?? null,
    role: role ? role : null,
    projectId: entry.projectId,
    projectName: project?.name ?? null,
    status: session.status,
    ageSeconds: seatAgeSeconds(session.statusAt, nowSeconds),
    packSha: session.packRef?.sha ?? null,
  };
}

function compareStrings(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

/** Freshest status first; no observed status last; ties by key, so the order is stable. */
function compareSeats(a: SeatRow, b: SeatRow): number {
  const ageA = a.ageSeconds ?? Number.POSITIVE_INFINITY;
  const ageB = b.ageSeconds ?? Number.POSITIVE_INFINITY;
  if (ageA !== ageB) return ageA < ageB ? -1 : 1;
  return compareStrings(a.key, b.key);
}

function agentChip(agent: ManagedAgent): RoleAgentChip {
  return {
    pubkey: agent.pubkey,
    name: agent.name,
    hasRolePack: agent.hasRolePack,
    packRefusedSharedHome: agent.packRefusedSharedHome,
    status: agent.status,
  };
}

function compareChips(a: RoleAgentChip, b: RoleAgentChip): number {
  return compareStrings(a.name, b.name) || compareStrings(a.pubkey, b.pubkey);
}

function roleRow(
  slug: string,
  pack: RolePackSummary | null,
  agents: RoleAgentChip[],
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
    seats,
  };
}

/** Join packs, agents, shelf and projects into the three lists the tab draws. */
export function buildRolesView(input: BuildRolesViewInput): RolesView {
  const { rolePacks, agents, shelfEntries, projects, nowSeconds } = input;
  const agentsByPubkey = new Map(
    agents.map((agent) => [agent.pubkey.toLowerCase(), agent] as const),
  );
  const projectsById = new Map(
    projects.map((project) => [project.id, project] as const),
  );
  // A closed or archived session is filed away by a shared closure fact; it
  // holds no seat, in any role or any project.
  const seats = shelfEntries
    .filter((entry) => !entry.isClosed)
    .map((entry) => seatRow(entry, agentsByPubkey, projectsById, nowSeconds))
    .sort(compareSeats);

  const packsByRole = new Map(rolePacks.map((pack) => [pack.role, pack]));
  const slugs = new Set<string>(packsByRole.keys());
  for (const agent of agents) {
    const role = homeRoleOf(agent);
    if (role) slugs.add(role);
  }
  for (const seat of seats) {
    if (seat.role) slugs.add(seat.role);
  }
  const roles = [...slugs].sort(compareStrings).map((slug) =>
    roleRow(
      slug,
      packsByRole.get(slug) ?? null,
      agents
        .filter((agent) => homeRoleOf(agent) === slug)
        .map(agentChip)
        .sort(compareChips),
      seats.filter((seat) => seat.role === slug),
    ),
  );

  const byProject = projects.map((project) => ({
    projectId: project.id,
    projectName: project.name,
    seats: seats.filter((seat) => seat.projectId === project.id),
  }));
  // Unplaced: no project claims the seat — and a seat claimed by a project
  // this list does not carry, which would otherwise vanish from a view that
  // promises every seat.
  const unplaced = seats.filter(
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
