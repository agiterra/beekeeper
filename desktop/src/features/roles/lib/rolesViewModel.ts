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
import type { ProjectCodingSessionShelfEntry } from "@/features/projects-container/lib/projectCodingSessionShelf";
import type { ProjectContainer } from "@/features/projects-container/lib/projectContainerModel";
import type {
  ManagedAgent,
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

function homeRoleOf(agent: ManagedAgent): string | null {
  const role = agent.homeRole?.trim();
  return role ? role : null;
}

function compareStrings(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
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
  const projectsById = new Map(
    projects.map((project) => [project.id, project] as const),
  );
  // A closed or archived session is filed away by a shared closure fact; it
  // holds no seat, in any role or any project (`includeClosed` defaults to
  // `false`). Sorted live-first, then freshest-first (Fix 1).
  const seats = buildSeatRows({ shelfEntries, agents, projects, nowSeconds });

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

/** What an agent chip on a role card discloses about that agent's pack (Fix 2). */
export type RolePackChipState = "refused" | "missing" | "blocked" | "present";

/**
 * The chip's pack disclosure comes from the *role row*, not the per-agent
 * probe (`agent.hasRolePack`): a role whose pack resolved should not print
 * "pack missing" on an agent just because nobody asked that agent's own
 * probe yet. In order: a shared home that refused the pack outranks
 * everything (it is a fact about this agent specifically); then a role with
 * no pack at all; then a role whose pack exists but is refused for another
 * reason (the row's own refusal sentence); otherwise the pack is present and
 * will be staged.
 */
export function roleAgentPackState(input: {
  roleHasPack: boolean;
  roleRefusal: string | null;
  agentPackRefusedSharedHome: boolean | undefined;
}): RolePackChipState {
  if (input.agentPackRefusedSharedHome === true) return "refused";
  if (input.roleHasPack === false) return "missing";
  if (input.roleRefusal !== null) return "blocked";
  return "present";
}
