/**
 * Read-only comparison between this machine's current role-pack resolution
 * and role-pack coordinates claimed by signed metadata in readable project
 * channels.
 *
 * A resolved pack is a snapshot made when the local packs query completed. A
 * metadata claim is one channel member's signed 44223 account of a generation.
 * Open ingress proves the signature and readable channel, but it does not prove
 * that the signer was commissioned as a provider. Neither side implies that
 * every machine resolved the same tree or that a running process changed.
 *
 * A running generation never changes packs; the next launch or resume adopts
 * the current project revision (provider behaviour, unchanged by this module,
 * cited as `crates/buzz-session-provider/src/lib.rs:3325-3332`).
 */
import type {
  CodingSessionStatus,
  GlobalCodingSessionCatalogRecord,
} from "@/features/coding-sessions/lib/codingSessionTypes";
import { PACK_REF_SHIPPED_REPO } from "@/features/coding-sessions/lib/codingSessionPackRef";
import type {
  ProjectPackRevisionComparison,
  RolePackSummary,
} from "@/shared/api/types";

import { seatAgeSeconds } from "./seatRows";

export type RolePackCoordinate = {
  repo: string;
  sha: string;
  role: string;
  path: string;
};

export type ResolvedRolePackSnapshot = {
  role: string;
  coordinate: RolePackCoordinate | null;
  reason: string | null;
};

/**
 * How a reported coordinate relates to this machine, widening the wire's
 * `ProjectPackRevisionRelation` with three outcomes the revision comparison
 * never needs to answer: `different-source` (the coordinate names a source
 * other than the project's current one — a foreign repository, or the
 * bundled defaults while the project names a repository), `shipped-differs`
 * (both sides are the bundled defaults, from different app versions) and
 * `incomplete` (no full coordinate at all).
 */
export type ReportedRolePackRelation =
  | ProjectPackRevisionComparison["relations"][number]["relation"]
  | "different-source"
  | "shipped-differs"
  | "incomplete";

/** Whether a running execution would keep its reported revision. */
export type ReportedRolePackAdoption = "keeps-until-next-generation" | "none";

export type ReportedRolePackSnapshot = {
  channelId: string;
  generationId: string;
  label: string;
  /** Signer of the 44223 metadata. Commissioning is not established here. */
  claimedByPubkey: string | null;
  reportedAt: number | null;
  coordinate: RolePackCoordinate | null;
  reason: string | null;
  relation: ReportedRolePackRelation;
  /** Commits this row is behind `HEAD`, set only when `relation` is `"earlier"`. */
  behind: number | null;
  /** Commits this row is ahead of `HEAD`, set only when `relation` is `"later"`. */
  ahead: number | null;
  /** The catalog record's status word, verbatim. */
  status: CodingSessionStatus;
  /** Seconds since `reportedAt`, or `null` when no status was observed. */
  ageSeconds: number | null;
  /**
   * `"keeps-until-next-generation"` when this row is on a non-current,
   * non-incomplete revision and the execution has not reached a terminal
   * status — the running or resting generation will keep this revision
   * until its next launch or resume. `"none"` otherwise.
   */
  adoption: ReportedRolePackAdoption;
  provenance: "unverified-channel-metadata";
};

export type RolePackSnapshots = {
  resolvedAt: number | null;
  resolved: ResolvedRolePackSnapshot[];
  reported: ReportedRolePackSnapshot[];
  /** The revision comparison's own header facts, or `null` when none ran. */
  comparison: {
    currentSha: string | null;
    comparedAt: number;
    reason: string | null;
  } | null;
};

/** Statuses a report can never advance from — no next generation is coming. */
const TERMINAL_STATUSES: ReadonlySet<CodingSessionStatus> = new Set([
  "completed",
  "stopped",
  "failed",
]);

function nonEmpty(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value : null;
}

/**
 * A portable coordinate needs all four values. The renderer intentionally
 * refuses partial shapes: inferring a path from the local directory would
 * make a machine-local fact look like shared provenance.
 */
export function readRolePackCoordinate(
  value: unknown,
): RolePackCoordinate | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return null;
  }
  const record = value as Record<string, unknown>;
  const repo = nonEmpty(record.repo);
  const sha = nonEmpty(record.sha);
  const role = nonEmpty(record.role);
  const path = nonEmpty(record.path);
  return repo && sha && role && path ? { repo, sha, role, path } : null;
}

function resolvedSnapshot(pack: RolePackSummary): ResolvedRolePackSnapshot {
  const coordinate = readRolePackCoordinate(pack.packRef);
  return {
    role: pack.role,
    coordinate,
    reason:
      coordinate !== null
        ? null
        : (pack.refusal ??
          "This machine did not return a complete portable coordinate."),
  };
}

function sameCoordinate(
  left: RolePackCoordinate,
  right: RolePackCoordinate,
): boolean {
  return (
    left.repo === right.repo &&
    left.sha === right.sha &&
    left.role === right.role &&
    left.path === right.path
  );
}

const NO_DISTANCE = { behind: null, ahead: null } as const;

/**
 * A reported coordinate's relation to this machine, plus its distance.
 *
 * The bundled defaults have no git history to rank, so a shipped coordinate
 * is compared with what this machine would stage for the same role: the
 * same app version is `current`, another is `shipped-differs`, and a role
 * this machine does not resolve from its own bundle is `unknown-here`. While
 * the project names a repository, a shipped claim is a `different-source`.
 * A repository claim can only be ranked once the project's source is known
 * (`sourceKnown`): a source read that failed leaves it `unknown-here`, never
 * "different", because nothing here knows what it would differ from.
 */
function revisionRelation(
  coordinate: RolePackCoordinate | null,
  input: {
    sourceRepo: string | null;
    sourceKnown: boolean;
    resolvedByRole: ReadonlyMap<string, RolePackCoordinate>;
    revisions: ProjectPackRevisionComparison | null;
  },
): Pick<ReportedRolePackSnapshot, "relation" | "behind" | "ahead"> {
  if (coordinate === null) {
    return { relation: "incomplete", ...NO_DISTANCE };
  }
  if (coordinate.repo === PACK_REF_SHIPPED_REPO) {
    if (input.sourceRepo !== null) {
      return { relation: "different-source", ...NO_DISTANCE };
    }
    const local = input.resolvedByRole.get(coordinate.role) ?? null;
    if (local === null || local.repo !== PACK_REF_SHIPPED_REPO) {
      return { relation: "unknown-here", ...NO_DISTANCE };
    }
    return {
      relation: sameCoordinate(local, coordinate)
        ? "current"
        : "shipped-differs",
      ...NO_DISTANCE,
    };
  }
  if (!input.sourceKnown) {
    return { relation: "unknown-here", ...NO_DISTANCE };
  }
  if (coordinate.repo !== input.sourceRepo) {
    return { relation: "different-source", ...NO_DISTANCE };
  }
  const { revisions } = input;
  const entry = revisions?.relations.find((row) => row.sha === coordinate.sha);
  if (entry === undefined) {
    return { relation: "unknown-here", behind: null, ahead: null };
  }
  return { relation: entry.relation, behind: entry.behind, ahead: entry.ahead };
}

function adoptionFor(
  relation: ReportedRolePackRelation,
  status: CodingSessionStatus,
): ReportedRolePackAdoption {
  if (relation === "current" || relation === "incomplete") return "none";
  return TERMINAL_STATUSES.has(status) ? "none" : "keeps-until-next-generation";
}

const FULL_SHA = /^[0-9a-f]{40}$/;

/**
 * The sorted, distinct 40-hex shas among `entries`' coordinates that name
 * `sourceRepo` — the set worth asking `compare_project_pack_revisions`
 * about. A coordinate naming a different repo (including `app:shipped`, or
 * any repo when `sourceRepo` is `null`) is excluded: this machine's checkout
 * cannot answer for a tree it never claims to hold, and a non-hex `sha`
 * (a shipped pack's version string) is never a valid git revision to ask
 * about.
 */
export function revisionShas(
  entries: readonly GlobalCodingSessionCatalogRecord[],
  sourceRepo: string | null,
): string[] {
  const shas = new Set<string>();
  if (sourceRepo === null) return [];
  for (const { session } of entries) {
    const coordinate = readRolePackCoordinate(session.packRef);
    if (
      coordinate !== null &&
      coordinate.repo === sourceRepo &&
      FULL_SHA.test(coordinate.sha)
    ) {
      shas.add(coordinate.sha);
    }
  }
  return [...shas].sort();
}

/**
 * Preserve every open-ingress metadata generation as an explicitly unverified
 * claim. The caller supplies only project-visible channels, and this second
 * project-ref check prevents a shared channel from lending another project's
 * claim to this comparison. Nothing here upgrades membership into provider
 * commissioning authority.
 */
export function buildRolePackSnapshots(input: {
  projectRef: string;
  resolvedAt: number | null;
  resolvedPacks: readonly RolePackSummary[];
  catalogEntries: readonly GlobalCodingSessionCatalogRecord[];
  /** The project's current source repo coordinate, or `null` when it names none. */
  sourceRepo: string | null;
  /**
   * Whether the project's source is actually known — a successful 30624 read
   * (possibly finding none). `false` means the read failed, and a repository
   * claim cannot be called different from a source nobody could read.
   */
  sourceKnown: boolean;
  /** The revision comparison's answer, or `null` before it has one. */
  revisions: ProjectPackRevisionComparison | null;
  /** The revision comparison's disclosed error, or `null`. */
  revisionsError: string | null;
  nowSeconds: number;
}): RolePackSnapshots {
  // A query in error can still carry stale `data` from an earlier success
  // (React Query does not clear it by default). Treat that as no comparison
  // at all — a row must never read `current` off an answer this call
  // couldn't reproduce.
  const revisions = input.revisionsError === null ? input.revisions : null;

  const resolved = input.resolvedPacks
    .map(resolvedSnapshot)
    .sort((left, right) => left.role.localeCompare(right.role));
  const resolvedByRole = new Map<string, RolePackCoordinate>();
  for (const snapshot of resolved) {
    if (snapshot.coordinate !== null) {
      resolvedByRole.set(snapshot.coordinate.role, snapshot.coordinate);
    }
  }

  const reported = input.catalogEntries
    .filter(({ session }) => session.projectRef === input.projectRef)
    .map(({ channelId, session }) => {
      const coordinate = readRolePackCoordinate(session.packRef);
      const { relation, behind, ahead } = revisionRelation(coordinate, {
        sourceRepo: input.sourceRepo,
        sourceKnown: input.sourceKnown,
        resolvedByRole,
        revisions,
      });
      return {
        channelId,
        generationId: session.generationId,
        label: session.title.trim() || session.label.trim() || "Metadata claim",
        claimedByPubkey: session.metadataAuthorityPubkey,
        reportedAt: session.statusAt,
        coordinate,
        reason:
          coordinate !== null
            ? null
            : "This metadata claim has no complete pack coordinate.",
        relation,
        behind,
        ahead,
        status: session.status,
        ageSeconds: seatAgeSeconds(session.statusAt, input.nowSeconds),
        adoption: adoptionFor(relation, session.status),
        provenance: "unverified-channel-metadata",
      } satisfies ReportedRolePackSnapshot;
    })
    .sort(
      (left, right) =>
        (right.reportedAt ?? -1) - (left.reportedAt ?? -1) ||
        left.channelId.localeCompare(right.channelId) ||
        left.generationId.localeCompare(right.generationId),
    );

  return {
    resolvedAt: input.resolvedAt,
    resolved,
    reported,
    comparison: revisions
      ? {
          currentSha: revisions.currentSha,
          comparedAt: revisions.comparedAt,
          reason: revisions.reason,
        }
      : null,
  };
}

export function rolePackCoordinateText(coordinate: RolePackCoordinate): string {
  return `${coordinate.repo}@${coordinate.sha} · ${coordinate.role} · ${coordinate.path}`;
}
