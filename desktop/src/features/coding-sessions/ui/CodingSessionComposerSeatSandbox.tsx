import * as React from "react";
import { LockOpen } from "lucide-react";

import {
  codingSessionEarlierUnsandboxedPeriod,
  type CodingSessionSandboxReport,
} from "@/features/coding-sessions/lib/codingSessionBoundaryStatus";
import { codingSessionUmbrellaExecutionParticipantKey } from "@/features/coding-sessions/lib/codingSessionUmbrellaComposerModel";

/**
 * Per-seat sandbox warnings for the mission view's recipient picker (SV-17).
 *
 * The boundary rows left every seat's transcript, and the composer's sandbox
 * chip follows only the addressed seat. Without this, a seat running with
 * full access is visible only to a viewer who happens to address it. So the
 * picker names every seat that runs, or ran in any generation, outside a
 * boundary — on the trigger itself for the seats it is not
 * showing, and on each seat's row.
 */
export type CodingSessionSeatSandboxWarning = {
  label: string;
  report: CodingSessionSandboxReport;
};

/**
 * The warning for one seat's boundary report, or `null` when it ran inside a
 * boundary throughout. An unreported boundary is not a warning here — it is
 * not called sandboxed anywhere either; the chip says "unreported" for it.
 */
export function codingSessionSeatSandboxWarning(
  report: CodingSessionSandboxReport,
): CodingSessionSeatSandboxWarning | null {
  if (report.state === "full-access") return { label: "Full access", report };
  if (report.state === "not-sandboxed") {
    return { label: "Not sandboxed", report };
  }
  const earlier = codingSessionEarlierUnsandboxedPeriod(report);
  if (!earlier) return null;
  return {
    label:
      earlier.state === "full-access" ? "Was full access" : "Was not sandboxed",
    report,
  };
}

/**
 * Every execution participant's warning, by participant key, from the seats'
 * boundary reports (`useCodingSessionExecutionSandboxes`, by execution key).
 * Those are read across every generation — the same reading the closed
 * Mission's footer makes — so a seat that ran with full access before a
 * resume says "Was full access" whether the Mission is open or closed.
 * Recomputed only when some seat's report changed, not per streamed item.
 */
export function useCodingSessionSeatSandboxWarnings(
  reports: ReadonlyMap<string, CodingSessionSandboxReport>,
): ReadonlyMap<string, CodingSessionSeatSandboxWarning> {
  return React.useMemo(() => {
    const warnings = new Map<string, CodingSessionSeatSandboxWarning>();
    for (const [executionKey, report] of reports) {
      const warning = codingSessionSeatSandboxWarning(report);
      if (warning) {
        warnings.set(
          codingSessionUmbrellaExecutionParticipantKey(executionKey),
          warning,
        );
      }
    }
    return warnings;
  }, [reports]);
}

/** The amber tag on a seat's row in the recipient picker. */
export function CodingSessionSeatSandboxTag({
  warning,
}: {
  warning: CodingSessionSeatSandboxWarning;
}) {
  return (
    <span
      className="inline-flex shrink-0 items-center gap-1 rounded-full border border-amber-500/50 bg-amber-500/10 px-1.5 text-2xs text-amber-700 dark:text-amber-300"
      data-testid="coding-session-seat-sandbox-warning"
      title={warning.report.boundaryText ?? undefined}
    >
      <LockOpen aria-hidden className="size-3" />
      {warning.label}
    </span>
  );
}

/** The accessible sentence for {@link CodingSessionOtherSeatsSandboxBadge}. */
export function codingSessionOtherSeatsSandboxLabel(count: number): string {
  return count === 1
    ? "1 other seat is not sandboxed"
    : `${count} other seats are not sandboxed`;
}

/**
 * The picker trigger's count of seats outside a boundary that the composer's
 * chip is not already showing — so the mission view says so without a click.
 */
export function CodingSessionOtherSeatsSandboxBadge({
  count,
}: {
  count: number;
}) {
  if (count === 0) return null;
  const label = codingSessionOtherSeatsSandboxLabel(count);
  return (
    <span
      className="inline-flex shrink-0 items-center gap-1 rounded-full border border-amber-500/50 bg-amber-500/10 px-1.5 text-2xs text-amber-700 dark:text-amber-300"
      data-testid="coding-session-other-seats-sandbox-warning"
      title={label}
    >
      <LockOpen aria-hidden className="size-3" />
      <span className="sr-only">{label}</span>
      <span aria-hidden>{count}</span>
    </span>
  );
}
