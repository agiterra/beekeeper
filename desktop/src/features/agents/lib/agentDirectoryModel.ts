import { friendlyAgentLastError } from "./friendlyAgentLastError";
/**
 * The Agents directory — one row per agent this build knows about (managed
 * here, or discoverable on the wire), joined with its seat history across
 * every project.
 *
 * **Project membership is association** (`ManagedAgent.projectRef`): the
 * project filter and the row's project label read it and nothing else. A
 * seat is a secondary fact ("seated in <project>"), and a setup journal
 * installation without the association is shown as a warning, not as
 * membership (`docs/PROJECT_AGENT_HIRING_IMPL.md`).
 *
 * `roles/lib/seatRows.ts` (owner: U2) already resolves a seat as a channel
 * generation joined against agents and projects; this module never re-derives
 * that join. It only groups seats by agent and folds the group into the
 * fields §A of the design names. A `SeatRow` alone cannot say whether it is
 * open or closed (see `seatRows.ts`'s own note on `BuildSeatRowsInput`), so
 * callers pass `openSeatKeys` — the same pattern `contributorsModel.ts` (U3)
 * uses — computed from the raw shelf entries before they became rows.
 */
import { resolveAgentCardModelLabel } from "./agentCardModelLabel";
import { isManagedAgentActive } from "./managedAgentControlActions";
import type { SeatRow } from "@/features/roles/lib/seatRows";
import { normalizeProjectCoordinate } from "@/shared/lib/projectAgentAssociation";
import { normalizePubkey, truncatePubkey } from "@/shared/lib/pubkey";
import type { ManagedAgent, RelayAgent } from "@/shared/api/types";

/** The `Model not known here` sentence for a wire-only agent (§A row field 2). */
export const AGENT_MODEL_NOT_KNOWN_HERE = "Model not known here";

/** One seat as the directory row/detail pane read it. */
export type AgentDirectorySeat = {
  key: string;
  channelId: string;
  generationId: string;
  projectId: string | null;
  projectName: string | null;
  role: string | null;
  status: SeatRow["status"];
  ageSeconds: number | null;
  packSha: string | null;
  isClosed: boolean;
};

/**
 * One project this agent was installed for, as a setup journal recorded it.
 *
 * Installation is neither participation nor membership: an agent installed a
 * minute ago holds no seat yet, and until its record carries the association
 * the lead cannot hire it. The facts are kept apart on the row
 * (`seatProjectIds`, `installedProjects`, `project`) so none is read as
 * another.
 */
export type AgentDirectoryInstallation = {
  /** `30621:<owner>:<d>` — the address the journal recorded, verbatim. */
  projectRef: string;
  /** The project container's `id`, when a listed project has this address. */
  projectId: string | null;
  /** That project's display name, when it is listed. */
  projectName: string | null;
  role: string;
};

/** The project a managed record is associated with. */
export type AgentDirectoryProjectAssociation = {
  /** Normalized `30621:<owner>:<d>`. */
  projectRef: string;
  /** The listed project's `id`, when this viewer lists it. */
  projectId: string | null;
  projectName: string | null;
};

/** The project fields the directory needs to resolve an installation. */
export type AgentDirectoryProject = {
  id: string;
  address: string;
  name: string;
};

export type AgentDirectoryRow = {
  pubkey: string;
  name: string;
  /** A managed agent exists here for this pubkey. */
  isInstalled: boolean;
  /** `status` is `running`/`deployed` on the managed record. `false` for a wire-only agent. */
  isRunning: boolean;
  /** Installed but not running. `false` for a wire-only agent. */
  isStopped: boolean;
  /** Last failure reported by the installed agent, formatted for display. */
  lastError?: string | null;
  /** The installed process differs from its saved configuration. */
  needsRestart?: boolean;
  modelLabel: string;
  homeRole: string | null;
  hasRolePack?: boolean;
  packRefusedSharedHome?: boolean;
  /** "lead ×12 · verifier ×3", or "no seats recorded". */
  roleHistoryLabel: string;
  /** Distinct role slugs from `homeRole`, seat history and installations, for filter options. */
  roleSlugs: string[];
  /** The freshest open seat, or `null` when this agent holds none right now. */
  currentSeat: AgentDirectorySeat | null;
  /**
   * The project this agent belongs to, from its managed record. `null` when
   * it belongs to none — or, for a wire-only agent, when this computer holds
   * no record to say (`projectKnown` is then `false`).
   */
  project: AgentDirectoryProjectAssociation | null;
  /** A managed record here answered the association question. */
  projectKnown: boolean;
  /**
   * The record here names no project but carries a `project_digest` its
   * owner published from another computer. Not membership on this one.
   */
  carriedFromAnotherComputer?: boolean;
  /** Every project this agent holds or held a seat in — not membership. */
  seatProjectIds: ReadonlySet<string>;
  /**
   * Every project this computer installed this agent for, from the setup
   * journals. Empty when none is recorded or the journals are not read yet.
   */
  installedProjects: AgentDirectoryInstallation[];
  /** `projectId`s of `installedProjects` that resolve to a listed project. */
  installedProjectIds: ReadonlySet<string>;
  /**
   * Installations whose agent record carries no association: the lead
   * cannot hire it for that project yet. Empty once associated.
   */
  unassociatedInstallations: AgentDirectoryInstallation[];
  /** Open first, then closed; freshest first within each group. */
  seats: AgentDirectorySeat[];
};

/**
 * "lead ×12 · verifier ×3" — groups sorted by count desc then slug asc, at
 * most three, then "· +N more". Empty input reads "no seats recorded".
 */
export function roleHistoryText(counts: ReadonlyMap<string, number>): string {
  if (counts.size === 0) return "no seats recorded";
  const groups = [...counts.entries()].sort(
    ([slugA, countA], [slugB, countB]) => {
      if (countA !== countB) return countB - countA;
      return slugA < slugB ? -1 : slugA > slugB ? 1 : 0;
    },
  );
  const shown = groups
    .slice(0, 3)
    .map(([slug, count]) => `${slug} ×${count}`)
    .join(" · ");
  const more = groups.length - 3;
  return more > 0 ? `${shown} · +${more} more` : shown;
}

function toDirectorySeat(
  seat: SeatRow,
  openSeatKeys: ReadonlySet<string>,
): AgentDirectorySeat {
  return {
    key: seat.key,
    channelId: seat.channelId,
    generationId: seat.generationId,
    projectId: seat.projectId,
    projectName: seat.projectName,
    role: seat.role,
    status: seat.status,
    ageSeconds: seat.ageSeconds,
    packSha: seat.packSha,
    isClosed: !openSeatKeys.has(seat.key),
  };
}

function orderSeats(seats: AgentDirectorySeat[]): AgentDirectorySeat[] {
  const byAge = (a: AgentDirectorySeat, b: AgentDirectorySeat) => {
    const ageA = a.ageSeconds ?? Number.POSITIVE_INFINITY;
    const ageB = b.ageSeconds ?? Number.POSITIVE_INFINITY;
    if (ageA !== ageB) return ageA - ageB;
    return a.key < b.key ? -1 : a.key > b.key ? 1 : 0;
  };
  const open = seats.filter((seat) => !seat.isClosed).sort(byAge);
  const closed = seats.filter((seat) => seat.isClosed).sort(byAge);
  return [...open, ...closed];
}

export type BuildAgentDirectoryInput = {
  managedAgents: readonly ManagedAgent[];
  relayAgents: readonly RelayAgent[];
  /** Every seat this agent set has ever held, across every project (`includeClosed: true`). */
  seats: readonly SeatRow[];
  /** Keys (`channelId/generationId`) of shelf entries that are not closed. */
  openSeatKeys: ReadonlySet<string>;
  defaultModel: string;
  /**
   * `installedProjectRefsByAgent(...)` from `roles/lib/projectInstalledRoles`:
   * lowercased agent pubkey → the project addresses it was installed for.
   */
  installationsByAgent?: ReadonlyMap<
    string,
    readonly { projectRef: string; role: string }[]
  >;
  /** The listed projects, used to turn an installation's address into an id. */
  projects?: readonly AgentDirectoryProject[];
};

/**
 * Key for comparing project addresses. The journal and the project container
 * both spell `30621:<owner>:<d>`; case is folded the same way the shared
 * installed-roles module folds it, so the two never disagree.
 */
function projectAddressKey(address: string): string {
  return address.trim().toLowerCase();
}

/**
 * Resolve each installation's address to a listed project.
 *
 * The directory's project filter speaks in `project.id` (`<owner>:<d>`), and
 * the journal records the address (`30621:<owner>:<d>`). The bridge is the
 * container's own `address` field — never string surgery on the ref, which
 * would invent an id for a project this viewer cannot see.
 */
export function resolveAgentInstallations(
  entries: readonly { projectRef: string; role: string }[],
  projects: readonly AgentDirectoryProject[],
): AgentDirectoryInstallation[] {
  const byAddress = new Map(
    projects.map(
      (project) => [projectAddressKey(project.address), project] as const,
    ),
  );
  return entries.map((entry) => {
    const project = byAddress.get(projectAddressKey(entry.projectRef)) ?? null;
    return {
      projectRef: entry.projectRef,
      projectId: project?.id ?? null,
      projectName: project?.name ?? null,
      role: entry.role,
    };
  });
}

export function buildAgentDirectory(
  input: BuildAgentDirectoryInput,
): AgentDirectoryRow[] {
  const { managedAgents, relayAgents, seats, openSeatKeys, defaultModel } =
    input;
  const installationsByAgent = input.installationsByAgent ?? new Map();
  const projects = input.projects ?? [];

  const managedByPubkey = new Map(
    managedAgents.map(
      (agent) => [normalizePubkey(agent.pubkey), agent] as const,
    ),
  );
  const relayByPubkey = new Map(
    relayAgents.map((agent) => [normalizePubkey(agent.pubkey), agent] as const),
  );
  const projectsByAddress = new Map<string, AgentDirectoryProject>();
  for (const listed of projects) {
    const ref = normalizeProjectCoordinate(listed.address);
    if (ref) projectsByAddress.set(ref, listed);
  }
  const pubkeys = new Set<string>([
    ...managedByPubkey.keys(),
    ...relayByPubkey.keys(),
  ]);

  const seatsByPubkey = new Map<string, SeatRow[]>();
  for (const seat of seats) {
    if (!seat.agentPubkey) continue;
    const key = normalizePubkey(seat.agentPubkey);
    const list = seatsByPubkey.get(key);
    if (list) {
      list.push(seat);
    } else {
      seatsByPubkey.set(key, [seat]);
    }
  }

  const rows: AgentDirectoryRow[] = [];
  for (const pubkey of pubkeys) {
    const managed = managedByPubkey.get(pubkey) ?? null;
    const relay = relayByPubkey.get(pubkey) ?? null;
    // A nameless agent falls back to the app's ONE compact pubkey form. Never
    // a hand-rolled prefix: a short prefix is forgeable by grinding, so the
    // shape a reader learns to recognise has to be the same everywhere.
    const name = managed?.name ?? relay?.name ?? truncatePubkey(pubkey);

    const rawSeats = seatsByPubkey.get(pubkey) ?? [];
    const directorySeats = orderSeats(
      rawSeats.map((seat) => toDirectorySeat(seat, openSeatKeys)),
    );
    const currentSeat = directorySeats.find((seat) => !seat.isClosed) ?? null;

    const roleCounts = new Map<string, number>();
    const seatProjectIds = new Set<string>();
    const roleSlugSet = new Set<string>();
    for (const seat of directorySeats) {
      if (seat.role) {
        roleCounts.set(seat.role, (roleCounts.get(seat.role) ?? 0) + 1);
        roleSlugSet.add(seat.role);
      }
      if (seat.projectId) seatProjectIds.add(seat.projectId);
    }

    const homeRole = managed?.homeRole?.trim() || null;
    if (homeRole) roleSlugSet.add(homeRole);

    const modelLabel = managed
      ? resolveAgentCardModelLabel({
          agent: managed,
          personaModel: undefined,
          defaultModel,
        })
      : AGENT_MODEL_NOT_KNOWN_HERE;

    const isRunning = managed ? isManagedAgentActive(managed) : false;

    const installedProjects = resolveAgentInstallations(
      installationsByAgent.get(pubkey) ?? [],
      projects,
    );
    const installedProjectIds = new Set<string>();
    for (const installation of installedProjects) {
      if (installation.projectId) {
        installedProjectIds.add(installation.projectId);
      }
      roleSlugSet.add(installation.role);
    }

    const associatedRef = normalizeProjectCoordinate(managed?.projectRef);
    let project: AgentDirectoryProjectAssociation | null = null;
    if (associatedRef) {
      const listed = projectsByAddress.get(associatedRef) ?? null;
      project = {
        projectRef: associatedRef,
        projectId: listed?.id ?? null,
        projectName: listed?.name ?? null,
      };
    }

    rows.push({
      pubkey,
      name,
      isInstalled: managed !== null,
      isRunning,
      isStopped: managed !== null && !isRunning,
      needsRestart: managed?.needsRestart ?? false,
      lastError: !isRunning
        ? (friendlyAgentLastError(
            managed?.lastError ?? null,
            managed?.lastErrorCode,
          )?.copy ?? null)
        : null,
      modelLabel,
      homeRole,
      hasRolePack: managed?.hasRolePack,
      packRefusedSharedHome: managed?.packRefusedSharedHome,
      roleHistoryLabel: roleHistoryText(roleCounts),
      roleSlugs: [...roleSlugSet].sort(),
      currentSeat,
      project,
      projectKnown: managed !== null,
      carriedFromAnotherComputer:
        managed !== null &&
        project === null &&
        (managed.carriedProjectDigest ?? "").trim() !== "",
      seatProjectIds,
      installedProjects,
      installedProjectIds,
      unassociatedInstallations:
        managed !== null && project === null ? installedProjects : [],
      seats: directorySeats,
    });
  }

  return rows.sort((a, b) => a.name.localeCompare(b.name));
}

export type AgentDirectoryStatusFilter =
  | "any"
  | "running"
  | "stopped"
  | "seated"
  | "not-seated"
  | "project-agent";

export type AgentDirectoryFilters = {
  role: string | null;
  status: AgentDirectoryStatusFilter;
  /** A project id, or `null` for "Any project". */
  projectId: string | null;
  /** "Installed on this computer" — checked by default. */
  installedOnly: boolean;
};

function projectKeyMatches(
  association: { projectRef: string; projectId: string | null },
  projectId: string,
): boolean {
  if (association.projectId === projectId) return true;
  const wanted = normalizeProjectCoordinate(projectId);
  return wanted !== null && association.projectRef === wanted;
}

/**
 * The installations of `row` that belong to `projectId`, or every one with
 * no project chosen.
 *
 * `projectId` is normally a container id (`<owner>:<d>`); an address
 * (`30621:<owner>:<d>`) is matched against the journal's own ref too, so a
 * caller holding either spelling gets the same answer.
 */
export function installationsForProject(
  row: Pick<AgentDirectoryRow, "installedProjects">,
  projectId: string | null,
): AgentDirectoryInstallation[] {
  if (!projectId) return row.installedProjects;
  const wantedAddress = projectAddressKey(projectId);
  return row.installedProjects.filter(
    (installation) =>
      installation.projectId === projectId ||
      projectAddressKey(installation.projectRef) === wantedAddress,
  );
}

/** Whether `row`'s record associates it with `projectId`. */
export function agentDirectoryRowAssociatedWith(
  row: Pick<AgentDirectoryRow, "project">,
  projectId: string,
): boolean {
  return row.project !== null && projectKeyMatches(row.project, projectId);
}

/**
 * True when `row` belongs under `projectId`: its record is associated with
 * that project, or it was installed for that project and its record carries
 * no association yet (listed with a warning, so the gap is visible where
 * people look for it). A seat, past or present, never places an agent.
 */
export function agentDirectoryRowInProject(
  row: Pick<
    AgentDirectoryRow,
    "project" | "unassociatedInstallations" | "installedProjects"
  >,
  projectId: string,
): boolean {
  if (agentDirectoryRowAssociatedWith(row, projectId)) return true;
  if (row.unassociatedInstallations.length === 0) return false;
  return (
    installationsForProject(
      { installedProjects: row.unassociatedInstallations },
      projectId,
    ).length > 0
  );
}

export function agentDirectoryFilter(
  rows: readonly AgentDirectoryRow[],
  filters: AgentDirectoryFilters,
): AgentDirectoryRow[] {
  return rows.filter((row) => {
    if (filters.installedOnly && !row.isInstalled) return false;
    if (filters.role && !row.roleSlugs.includes(filters.role)) return false;
    if (
      filters.projectId &&
      !agentDirectoryRowInProject(row, filters.projectId)
    ) {
      return false;
    }
    switch (filters.status) {
      case "running":
        return row.isRunning;
      case "stopped":
        return row.isStopped;
      case "seated":
        return row.currentSeat !== null;
      case "not-seated":
        return row.currentSeat === null;
      case "project-agent":
        return filters.projectId
          ? agentDirectoryRowAssociatedWith(row, filters.projectId)
          : row.project !== null;
      default:
        return true;
    }
  });
}
