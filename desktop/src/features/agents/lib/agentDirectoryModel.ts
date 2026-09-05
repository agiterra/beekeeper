/**
 * The Agents directory — one row per agent this build knows about (managed
 * here, or discoverable on the wire), joined with its seat history across
 * every project.
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

export type AgentDirectoryRow = {
  pubkey: string;
  name: string;
  /** A managed agent exists here for this pubkey. */
  isInstalled: boolean;
  /** `status` is `running`/`deployed` on the managed record. `false` for a wire-only agent. */
  isRunning: boolean;
  /** Installed but not running. `false` for a wire-only agent. */
  isStopped: boolean;
  modelLabel: string;
  homeRole: string | null;
  hasRolePack?: boolean;
  packRefusedSharedHome?: boolean;
  /** "lead ×12 · verifier ×3", or "no seats recorded". */
  roleHistoryLabel: string;
  /** Distinct role slugs from `homeRole` and seat history, for filter options. */
  roleSlugs: string[];
  /** The freshest open seat, or `null` when this agent holds none right now. */
  currentSeat: AgentDirectorySeat | null;
  /** Every project this agent holds or held a seat in. */
  seatProjectIds: ReadonlySet<string>;
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
};

export function buildAgentDirectory(
  input: BuildAgentDirectoryInput,
): AgentDirectoryRow[] {
  const { managedAgents, relayAgents, seats, openSeatKeys, defaultModel } =
    input;

  const managedByPubkey = new Map(
    managedAgents.map(
      (agent) => [normalizePubkey(agent.pubkey), agent] as const,
    ),
  );
  const relayByPubkey = new Map(
    relayAgents.map((agent) => [normalizePubkey(agent.pubkey), agent] as const),
  );
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

    rows.push({
      pubkey,
      name,
      isInstalled: managed !== null,
      isRunning,
      isStopped: managed !== null && !isRunning,
      modelLabel,
      homeRole,
      hasRolePack: managed?.hasRolePack,
      packRefusedSharedHome: managed?.packRefusedSharedHome,
      roleHistoryLabel: roleHistoryText(roleCounts),
      roleSlugs: [...roleSlugSet].sort(),
      currentSeat,
      seatProjectIds,
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
  | "not-seated";

export type AgentDirectoryFilters = {
  role: string | null;
  status: AgentDirectoryStatusFilter;
  /** A project id, or `null` for "Any project". */
  projectId: string | null;
  /** "Installed on this computer" — checked by default. */
  installedOnly: boolean;
};

export function agentDirectoryFilter(
  rows: readonly AgentDirectoryRow[],
  filters: AgentDirectoryFilters,
): AgentDirectoryRow[] {
  return rows.filter((row) => {
    if (filters.installedOnly && !row.isInstalled) return false;
    if (filters.role && !row.roleSlugs.includes(filters.role)) return false;
    if (filters.projectId && !row.seatProjectIds.has(filters.projectId)) {
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
      default:
        return true;
    }
  });
}
