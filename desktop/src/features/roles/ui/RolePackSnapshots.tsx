import * as React from "react";
import { Button } from "@/shared/ui/button";
import { truncatePubkey } from "@/shared/lib/pubkey";
import type {
  RolePackCoordinate,
  RolePackSnapshots as RolePackSnapshotModel,
} from "../lib/rolePackSnapshots";
import { rolePackCoordinateText } from "../lib/rolePackSnapshots";
import type { RolePackProvenanceState } from "../lib/rolePackProvenance";
import {
  ADOPTION_KEEPS_UNTIL_NEXT_GENERATION,
  checkoutAnsweredSentence,
  provenanceLabelText,
  provenanceNotesSentence,
  reportedAgoText,
  reportHistorySummary,
  revisionComparisonUnavailableSentence,
  revisionRelationText,
  ROLE_PACK_SNAPSHOTS_SUBTITLE,
  ROLES_DIAGNOSTICS_CHECKS_TITLE,
  ROLES_DIAGNOSTICS_SOURCE_TITLE,
  ROLES_EXPLANATION,
  ROLES_TECHNICAL_DETAILS_TITLE,
} from "./rolesCopy";

/** Amber for unproven, red for a contradiction, plain for a commissioned
 * report — never emphasize `commissioned` the way an error state is. */
function provenanceLabelClassName(state: RolePackProvenanceState): string {
  switch (state) {
    case "commissioned":
      return "font-medium text-foreground";
    case "proof-unavailable":
      return "font-medium text-amber-600 dark:text-amber-400";
    case "disputed":
      return "font-medium text-destructive";
  }
}

const GROUP_SUMMARY_CLASS =
  "cursor-pointer text-xs font-medium text-foreground";

function timestamp(value: number | null): string {
  return value === null
    ? "time not reported"
    : new Date(value).toLocaleString();
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
 * The "Available here" heading's own commit fact: which commit git answered
 * for, and when — the reason it could not answer, or the revision
 * comparison's own disclosed error. Never renders a checkout as current
 * while the comparison failed (a stale successful answer never reaches this
 * component: `buildRolePackSnapshots` nulls it out itself).
 */
function CheckoutStatus({
  comparison,
  revisionsError,
}: {
  comparison: RolePackSnapshotModel["comparison"];
  revisionsError: string | null;
}) {
  if (comparison) {
    return (
      <p
        className="text-xs text-muted-foreground"
        data-testid="role-pack-checkout-status"
      >
        {comparison.currentSha
          ? checkoutAnsweredSentence(
              comparison.currentSha,
              timestamp(comparison.comparedAt),
            )
          : (comparison.reason ?? "This machine's checkout has no commit yet.")}
      </p>
    );
  }
  if (revisionsError) {
    return (
      <p
        className="text-xs text-amber-600 dark:text-amber-400"
        data-testid="role-pack-checkout-status"
      >
        {revisionComparisonUnavailableSentence(revisionsError)}
      </p>
    );
  }
  return null;
}

/** What the sessions shelf could and could not read, as it words it. */
export type RolesShelfNotice = {
  kind: string;
  message: string;
  detail?: string | null;
};

/** The coordinates, commits and rows this machine actually read. */
function SourceGroup({
  snapshots,
  resolvedError,
  resolvedIsStale,
  revisionsError,
}: {
  snapshots: RolePackSnapshotModel;
  resolvedError: string | null;
  resolvedIsStale: boolean;
  revisionsError: string | null;
}) {
  return (
    <details
      className="border-l border-border/60 pl-3"
      data-testid="roles-diagnostics-source"
    >
      <summary className={GROUP_SUMMARY_CLASS}>
        {ROLES_DIAGNOSTICS_SOURCE_TITLE}
      </summary>
      <div
        className="mt-1 flex flex-col gap-1"
        data-testid="role-pack-resolved"
      >
        <h3 className="text-2xs font-medium uppercase tracking-wide text-muted-foreground">
          Available here
        </h3>
        <p className="text-xs text-muted-foreground">
          {snapshots.resolvedAt === null
            ? "No successful local version check has been recorded in this view."
            : `Checked at ${timestamp(snapshots.resolvedAt)}.`}
        </p>
        <CheckoutStatus
          comparison={snapshots.comparison}
          revisionsError={revisionsError}
        />
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
    </details>
  );
}

/** One reported row, unchanged in structure from the pre-disclosure list. */
function ReportedRow({
  snapshot,
}: {
  snapshot: RolePackSnapshotModel["reported"][number];
}) {
  return (
    <li
      className="flex flex-col gap-1 text-xs"
      data-provenance={snapshot.provenance}
      data-relation={snapshot.relation}
      data-testid="role-pack-reported-row"
    >
      <p>
        <span className={provenanceLabelClassName(snapshot.provenance)}>
          {provenanceLabelText(snapshot.provenance, snapshot.provenanceReason)}
        </span>
        {" from "}
        <span
          className="font-mono text-muted-foreground"
          title={snapshot.claimedByPubkey ?? "Metadata signer unavailable"}
        >
          {snapshot.claimedByPubkey
            ? truncatePubkey(snapshot.claimedByPubkey)
            : "unknown signer"}
        </span>
        {" · "}
        <span className="font-medium text-foreground">{snapshot.label}</span>
        {" · "}
        {snapshot.coordinate ? (
          <span className="font-mono text-muted-foreground">
            {snapshot.coordinate.role} {versionSummary(snapshot.coordinate)}
          </span>
        ) : (
          <span className="text-muted-foreground">{snapshot.reason}</span>
        )}
        {" · "}
        <span className="text-muted-foreground">
          {revisionRelationText(
            snapshot.relation,
            snapshot.behind,
            snapshot.ahead,
          )}
        </span>
        {snapshot.note ? (
          <>
            {" · "}
            <span className="text-muted-foreground">{snapshot.note}</span>
          </>
        ) : null}
        {" · "}
        <span className="text-muted-foreground">
          {reportedAgoText(snapshot.ageSeconds)}
        </span>
        {" · "}
        <span className="text-muted-foreground">{snapshot.status}</span>
        {snapshot.adoption === "keeps-until-next-generation" ? (
          <>
            {" · "}
            <span className="text-muted-foreground">
              {ADOPTION_KEEPS_UNTIL_NEXT_GENERATION}
            </span>
          </>
        ) : null}
      </p>
      {snapshot.coordinate ? (
        <VersionDetails
          coordinate={snapshot.coordinate}
          generationId={snapshot.generationId}
        />
      ) : null}
    </li>
  );
}

/** Every report this project's channels carried, 25 rows at a time. */
function HistoryGroup({
  snapshots,
  reports,
}: {
  snapshots: RolePackSnapshotModel;
  reports: {
    isLoading: boolean;
    error: string | null;
    authorityError: string | null;
  };
}) {
  const [page, setPage] = React.useState(0);
  const pageSize = 25;
  const lastPage = Math.max(
    0,
    Math.ceil(snapshots.reported.length / pageSize) - 1,
  );
  const currentPage = Math.min(page, lastPage);
  const visibleReports = snapshots.reported.slice(
    currentPage * pageSize,
    (currentPage + 1) * pageSize,
  );
  return (
    <details
      className="border-l border-border/60 pl-3"
      data-testid="roles-diagnostics-history"
    >
      <summary className={GROUP_SUMMARY_CLASS}>
        {reportHistorySummary(snapshots.reported.length)}
      </summary>
      <div
        className="mt-1 flex flex-col gap-1"
        data-testid="role-pack-reported"
      >
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
            {visibleReports.map((snapshot) => (
              <ReportedRow
                key={`${snapshot.channelId}:${snapshot.generationId}`}
                snapshot={snapshot}
              />
            ))}
          </ul>
        )}
        {snapshots.reported.length > pageSize ? (
          <nav
            aria-label="Report history pages"
            className="flex items-center gap-3 text-sm"
          >
            <Button
              variant="outline"
              disabled={currentPage === 0}
              onClick={() => setPage(currentPage - 1)}
            >
              Previous
            </Button>
            <span>
              {currentPage * pageSize + 1}–
              {Math.min(
                (currentPage + 1) * pageSize,
                snapshots.reported.length,
              )}{" "}
              of {snapshots.reported.length} reports
            </span>
            <Button
              variant="outline"
              disabled={currentPage === lastPage}
              onClick={() => setPage(currentPage + 1)}
            >
              Next
            </Button>
          </nav>
        ) : null}
      </div>
    </details>
  );
}

/** What Beekeeper checked about the reports, and what it could not read. */
function ChecksGroup({
  snapshots,
  shelfNotice,
}: {
  snapshots: RolePackSnapshotModel;
  shelfNotice: RolesShelfNotice | null;
}) {
  return (
    <details
      className="border-l border-border/60 pl-3"
      data-testid="roles-diagnostics-checks"
    >
      <summary className={GROUP_SUMMARY_CLASS}>
        {ROLES_DIAGNOSTICS_CHECKS_TITLE}
      </summary>
      <div className="mt-1 flex flex-col gap-1">
        <p
          className="text-xs text-muted-foreground"
          data-testid="roles-explanation"
        >
          {ROLES_EXPLANATION}
        </p>
        <p className="text-xs text-muted-foreground">
          {ROLE_PACK_SNAPSHOTS_SUBTITLE}
        </p>
        {snapshots.provenanceNotes.length > 0 ? (
          <p
            className="text-xs text-muted-foreground"
            data-testid="role-pack-provenance-notes"
          >
            {provenanceNotesSentence(snapshots.provenanceNotes)}
          </p>
        ) : null}
        {shelfNotice ? (
          <p
            className="text-xs text-muted-foreground"
            data-shelf-state={shelfNotice.kind}
            data-testid="roles-shelf-notice"
          >
            {shelfNotice.message}
            {shelfNotice.detail ? ` — ${shelfNotice.detail}` : null}
          </p>
        ) : null}
      </div>
    </details>
  );
}

/**
 * Technical details: the coordinates, commits, reports and limitations
 * behind the sentences on the cards above.
 *
 * Everything here is collapsed by default and keyboard-operable natively
 * (`<details>`/`<summary>`), because none of it is what a reader opening the
 * tab is asking. Nothing is softened on the way in: the rows, ids and the
 * 25-row pagination are the same ones the flat section carried, and the
 * protocol's own long labels stay verbatim.
 */
export function RolePackSnapshots({
  snapshots,
  resolvedError,
  resolvedIsStale,
  revisionsError,
  reports,
  shelfNotice = null,
}: {
  snapshots: RolePackSnapshotModel;
  resolvedError: string | null;
  resolvedIsStale: boolean;
  revisionsError: string | null;
  reports: {
    isLoading: boolean;
    error: string | null;
    authorityError: string | null;
  };
  shelfNotice?: RolesShelfNotice | null;
}) {
  return (
    <section
      className="rounded-lg border border-border/70"
      data-testid="role-pack-snapshots"
    >
      <details data-testid="roles-technical-details">
        <summary className="cursor-pointer px-3 py-2 text-sm font-medium text-foreground">
          {ROLES_TECHNICAL_DETAILS_TITLE}
        </summary>
        <div className="flex flex-col gap-3 px-3 pb-3">
          <p className="text-xs text-muted-foreground">
            Sender confirmation checks the founder or an operator’s authority on
            the recorded grant timeline at each command’s signed timestamp. It
            does not prove which instructions executed or their real-time order.
          </p>
          <SourceGroup
            resolvedError={resolvedError}
            resolvedIsStale={resolvedIsStale}
            revisionsError={revisionsError}
            snapshots={snapshots}
          />
          <HistoryGroup reports={reports} snapshots={snapshots} />
          <ChecksGroup shelfNotice={shelfNotice} snapshots={snapshots} />
        </div>
      </details>
    </section>
  );
}
