import { truncatePubkey } from "@/shared/lib/pubkey";
import type {
  RolePackCoordinate,
  RolePackSnapshots as RolePackSnapshotModel,
} from "../lib/rolePackSnapshots";
import { rolePackCoordinateText } from "../lib/rolePackSnapshots";

function timestamp(value: number | null): string {
  return value === null
    ? "time not reported"
    : new Date(value).toLocaleString();
}

function comparisonText(
  value: "claims-match" | "claims-differ" | "unknown",
): string {
  switch (value) {
    case "claims-match":
      return "Claims same version";
    case "claims-differ":
      return "Claims different version";
    case "unknown":
      return "Version claim incomplete";
  }
}

function versionSummary(coordinate: RolePackCoordinate): string {
  return coordinate.sha.slice(0, 8);
}

function VersionDetails({
  coordinate,
  generationId,
}: {
  coordinate: RolePackCoordinate;
  generationId?: string;
}) {
  return (
    <details className="text-2xs text-muted-foreground">
      <summary className="cursor-pointer">Version details</summary>
      <p className="mt-1 break-all font-mono">
        {rolePackCoordinateText(coordinate)}
      </p>
      {generationId ? (
        <p className="mt-1 break-all font-mono">
          Claimed generation: {generationId}
        </p>
      ) : null}
    </details>
  );
}

/**
 * Project-scoped local version facts beside visibly unverified channel metadata
 * claims. Open ingress proves a signature, not provider commissioning.
 */
export function RolePackSnapshots({
  snapshots,
  resolvedError,
  resolvedIsStale,
  reports,
}: {
  snapshots: RolePackSnapshotModel;
  resolvedError: string | null;
  resolvedIsStale: boolean;
  reports: {
    isLoading: boolean;
    error: string | null;
    authorityError: string | null;
  };
}) {
  return (
    <section
      className="flex flex-col gap-3 rounded-lg border border-border/70 p-3"
      data-testid="role-pack-snapshots"
    >
      <div>
        <h2 className="text-sm font-medium text-foreground">
          Revision snapshots
        </h2>
        <p className="text-xs text-muted-foreground">
          Versions found on this machine and signed metadata claims visible in
          this project&rsquo;s channels. Beekeeper has not verified that a
          commissioned provider authored these claims.
        </p>
      </div>

      <div className="flex flex-col gap-1" data-testid="role-pack-resolved">
        <h3 className="text-2xs font-medium uppercase tracking-wide text-muted-foreground">
          Available here
        </h3>
        <p className="text-xs text-muted-foreground">
          {snapshots.resolvedAt === null
            ? "No successful local version check has been recorded in this view."
            : `Checked at ${timestamp(snapshots.resolvedAt)}.`}
        </p>
        {resolvedError ? (
          <p
            className="text-xs text-amber-600 dark:text-amber-400"
            data-testid="role-pack-resolved-error"
          >
            {resolvedIsStale
              ? `The last refresh failed; rows below are from the earlier check: ${resolvedError}`
              : `Version check unavailable: ${resolvedError}`}
          </p>
        ) : null}
        {snapshots.resolved.length === 0 ? (
          <p className="text-xs text-muted-foreground">
            No role versions are available here.
          </p>
        ) : (
          <ul className="flex flex-col gap-2">
            {snapshots.resolved.map((snapshot) => (
              <li
                className="flex flex-col gap-1 text-xs"
                data-testid="role-pack-resolved-row"
                key={snapshot.role}
              >
                <p>
                  <span className="font-medium text-foreground">
                    {snapshot.role}
                  </span>
                  {" · "}
                  {snapshot.coordinate ? (
                    <span className="font-mono text-muted-foreground">
                      {versionSummary(snapshot.coordinate)}
                    </span>
                  ) : (
                    <span className="text-muted-foreground">
                      {snapshot.reason}
                    </span>
                  )}
                </p>
                {snapshot.coordinate ? (
                  <VersionDetails coordinate={snapshot.coordinate} />
                ) : null}
              </li>
            ))}
          </ul>
        )}
      </div>

      <div className="flex flex-col gap-1" data-testid="role-pack-reported">
        <h3 className="text-2xs font-medium uppercase tracking-wide text-muted-foreground">
          Unverified channel metadata
        </h3>
        {reports.authorityError ? (
          <p
            className="text-xs text-amber-600 dark:text-amber-400"
            data-testid="role-pack-reports-unavailable"
          >
            Metadata claims unavailable: {reports.authorityError}
          </p>
        ) : reports.error ? (
          <p
            className="text-xs text-amber-600 dark:text-amber-400"
            data-testid="role-pack-reports-partial"
          >
            Metadata claims may be incomplete: {reports.error}
          </p>
        ) : reports.isLoading ? (
          <p className="text-xs text-muted-foreground">
            Reading metadata claims…
          </p>
        ) : null}
        {snapshots.reported.length === 0 ? (
          <p
            className="text-xs text-muted-foreground"
            data-testid="role-pack-reports-empty"
          >
            {reports.authorityError || reports.error
              ? "No complete metadata-claim list is available while this read has an error."
              : reports.isLoading
                ? "Metadata claims will appear when this read completes."
                : "No role-version metadata claims are visible for this project."}
          </p>
        ) : (
          <ul className="flex flex-col gap-2">
            {snapshots.reported.map((snapshot) => (
              <li
                className="flex flex-col gap-1 text-xs"
                data-comparison={snapshot.comparison}
                data-testid="role-pack-reported-row"
                key={`${snapshot.channelId}:${snapshot.generationId}`}
              >
                <p>
                  <span className="font-medium text-amber-600 dark:text-amber-400">
                    Unverified metadata
                  </span>
                  {" from "}
                  <span
                    className="font-mono text-muted-foreground"
                    title={
                      snapshot.claimedByPubkey ?? "Metadata signer unavailable"
                    }
                  >
                    {snapshot.claimedByPubkey
                      ? truncatePubkey(snapshot.claimedByPubkey)
                      : "unknown signer"}
                  </span>
                  {" · "}
                  <span className="font-medium text-foreground">
                    {snapshot.label}
                  </span>
                  {" · "}
                  {snapshot.coordinate ? (
                    <span className="font-mono text-muted-foreground">
                      {snapshot.coordinate.role}{" "}
                      {versionSummary(snapshot.coordinate)}
                    </span>
                  ) : (
                    <span className="text-muted-foreground">
                      {snapshot.reason}
                    </span>
                  )}
                  {" · "}
                  <span className="text-muted-foreground">
                    {comparisonText(snapshot.comparison)}
                  </span>
                  {" · reported "}
                  <span className="text-muted-foreground">
                    {timestamp(snapshot.reportedAt)}
                  </span>
                </p>
                {snapshot.coordinate ? (
                  <VersionDetails
                    coordinate={snapshot.coordinate}
                    generationId={snapshot.generationId}
                  />
                ) : null}
              </li>
            ))}
          </ul>
        )}
      </div>
    </section>
  );
}
