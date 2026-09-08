/**
 * Seat rows — a session generation as the Roles/Contributors surfaces read
 * it — and the pure ordering rules that decide what shows first.
 *
 * Split out of `rolesViewModel.ts` so both the project Packs view (open
 * seats only) and the project Contributors view (open and closed) build the
 * same row shape from the same shelf, agents and project lists, without
 * agreeing on a join by copying it.
 */
import type { CodingSessionStatus } from "@/features/coding-sessions/lib/codingSessionTypes";
import type { ProjectCodingSessionShelfEntry } from "@/features/projects-container/lib/projectCodingSessionShelf";
import type { ProjectContainer } from "@/features/projects-container/lib/projectContainerModel";
import type { ManagedAgent, RelayAgent } from "@/shared/api/types";

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
  /** Local managed name, then relay identity name, or `null` when unknown. */
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

/**
 * A seat is authorization and assignment, not proof of work — a live status
 * is the only thing that can call it "live" (ruling §1). These are the
 * catalog words a running provider can actually report.
 */
export function seatIsLive(status: CodingSessionStatus): boolean {
  return (
    status === "starting" ||
    status === "idle" ||
    status === "running" ||
    status === "waiting_for_input"
  );
}

/** Live seats first, then the existing freshest-first order within each half. */
export function compareSeatsLiveFirst(a: SeatRow, b: SeatRow): number {
  const liveA = seatIsLive(a.status);
  const liveB = seatIsLive(b.status);
  if (liveA !== liveB) return liveA ? -1 : 1;
  return compareSeats(a, b);
}

function seatRow(
  entry: ProjectCodingSessionShelfEntry,
  agentsByPubkey: ReadonlyMap<string, ManagedAgent>,
  projectsById: ReadonlyMap<string, ProjectContainer>,
  relayAgentsByPubkey: ReadonlyMap<string, RelayAgent>,
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
    agentName:
      agent?.name ??
      (agentPubkey ? relayAgentsByPubkey.get(agentPubkey)?.name : null) ??
      null,
    role: role ? role : null,
    projectId: entry.projectId,
    projectName: project?.name ?? null,
    status: session.status,
    ageSeconds: seatAgeSeconds(session.statusAt, nowSeconds),
    packSha: session.packRef?.sha ?? null,
  };
}

export type BuildSeatRowsInput = {
  shelfEntries: readonly ProjectCodingSessionShelfEntry[];
  agents: readonly ManagedAgent[];
  /** Shared identity fallback only; never changes a session's authority or status. */
  relayAgents?: readonly RelayAgent[];
  projects: readonly ProjectContainer[];
  nowSeconds: number;
  /**
   * `true` keeps closed/archived entries as seat rows too (Contributors, which
   * reads seat history). Default `false` matches `buildRolesView`'s existing
   * rule: a closed or archived session holds no seat, in any role or project.
   */
  includeClosed?: boolean;
};

/**
 * Join shelf entries, agents and projects into seat rows, sorted live-first
 * then freshest-first (Fix 1).
 */
export function buildSeatRows(input: BuildSeatRowsInput): SeatRow[] {
  const {
    shelfEntries,
    agents,
    relayAgents = [],
    projects,
    nowSeconds,
    includeClosed = false,
  } = input;
  const agentsByPubkey = new Map(
    agents.map((agent) => [agent.pubkey.toLowerCase(), agent] as const),
  );
  const relayAgentsByPubkey = new Map(
    relayAgents.map((agent) => [agent.pubkey.toLowerCase(), agent] as const),
  );
  const projectsById = new Map(
    projects.map((project) => [project.id, project] as const),
  );
  const entries = includeClosed
    ? shelfEntries
    : shelfEntries.filter((entry) => !entry.isClosed);
  return entries
    .map((entry) =>
      seatRow(
        entry,
        agentsByPubkey,
        projectsById,
        relayAgentsByPubkey,
        nowSeconds,
      ),
    )
    .sort(compareSeatsLiveFirst);
}
