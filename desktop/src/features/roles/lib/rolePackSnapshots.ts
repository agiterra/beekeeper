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
 */
import type { GlobalCodingSessionCatalogRecord } from "@/features/coding-sessions/lib/codingSessionTypes";
import type { RolePackSummary } from "@/shared/api/types";

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

export type ReportedRolePackSnapshot = {
  channelId: string;
  generationId: string;
  label: string;
  /** Signer of the 44223 metadata. Commissioning is not established here. */
  claimedByPubkey: string | null;
  reportedAt: number | null;
  coordinate: RolePackCoordinate | null;
  reason: string | null;
  comparison: "claims-match" | "claims-differ" | "unknown";
  provenance: "unverified-channel-metadata";
};

export type RolePackSnapshots = {
  resolvedAt: number | null;
  resolved: ResolvedRolePackSnapshot[];
  reported: ReportedRolePackSnapshot[];
};

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
}): RolePackSnapshots {
  const resolved = input.resolvedPacks
    .map(resolvedSnapshot)
    .sort((left, right) => left.role.localeCompare(right.role));
  const resolvedByRole = new Map(
    resolved
      .filter(
        (
          snapshot,
        ): snapshot is ResolvedRolePackSnapshot & {
          coordinate: RolePackCoordinate;
        } => snapshot.coordinate !== null,
      )
      .map((snapshot) => [snapshot.coordinate.role, snapshot.coordinate]),
  );

  const reported = input.catalogEntries
    .filter(({ session }) => session.projectRef === input.projectRef)
    .map(({ channelId, session }) => {
      const coordinate = readRolePackCoordinate(session.packRef);
      const resolvedCoordinate = coordinate
        ? (resolvedByRole.get(coordinate.role) ?? null)
        : null;
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
        comparison:
          coordinate === null || resolvedCoordinate === null
            ? "unknown"
            : sameCoordinate(coordinate, resolvedCoordinate)
              ? "claims-match"
              : "claims-differ",
        provenance: "unverified-channel-metadata",
      } satisfies ReportedRolePackSnapshot;
    })
    .sort(
      (left, right) =>
        (right.reportedAt ?? -1) - (left.reportedAt ?? -1) ||
        left.channelId.localeCompare(right.channelId) ||
        left.generationId.localeCompare(right.generationId),
    );

  return { resolvedAt: input.resolvedAt, resolved, reported };
}

export function rolePackCoordinateText(coordinate: RolePackCoordinate): string {
  return `${coordinate.repo}@${coordinate.sha} · ${coordinate.role} · ${coordinate.path}`;
}
