import * as React from "react";
import { CircleHelp, LoaderCircle } from "lucide-react";

import type {
  CodingSessionLandingGateRow,
  CodingSessionLandingRunningLine,
} from "@/features/coding-sessions/lib/codingSessionLandingModel";
import { cn } from "@/shared/lib/cn";

import { CodingSessionGateRows } from "./CodingSessionGateRows";

/**
 * The Landing Gate row's body (SV-41 S3): each gate the provider signed as
 * started sits directly above that gate's finished row, or alone when the
 * gate has no finished row yet; rows naming no commit follow under their
 * own heading. Every running line carries its start event, copyable, so a
 * reader can find it on the wire — the same affordance the Audit tab's
 * gate-start rows give.
 */
export function CodingSessionLandingGateBody({
  gate,
}: {
  gate: CodingSessionLandingGateRow;
}) {
  const groups = React.useMemo(() => groupRunningLines(gate), [gate]);
  const named = new Set(gate.rows.map((row) => row.gate));
  // Lines lead the first row of their gate: the named row when there is
  // one, else the first commitless row.
  const leadFor =
    (unnamed: boolean) =>
    (
      row: { gate: string },
      index: number,
      rows: readonly { gate: string }[],
    ) => {
      if (rows.findIndex((candidate) => candidate.gate === row.gate) !== index)
        return null;
      if (unnamed && named.has(row.gate)) return null;
      const lines = groups.byGate.get(row.gate) ?? [];
      return lines.map((line) => <RunningLine key={line.key} line={line} />);
    };
  return (
    <>
      {groups.alone.length > 0 ? (
        <ul
          aria-label="Gates signed as started"
          className="space-y-1"
          data-testid="coding-session-landing-gate-running"
        >
          {groups.alone.map((line) => (
            <RunningLine key={line.key} line={line} />
          ))}
        </ul>
      ) : null}
      {gate.rows.length > 0 ? (
        <CodingSessionGateRows
          leadFor={leadFor(false)}
          rows={gate.rows}
          testId="coding-session-landing-gate-rows"
        />
      ) : null}
      {gate.unnamedRows.length > 0 ? (
        <div
          className="space-y-1"
          data-testid="coding-session-landing-gate-unnamed"
        >
          <p
            className="text-2xs font-medium text-muted-foreground"
            title="Rows that name no commit speak for no head, so they never decide which commit Landed looks for. They are listed so a failure is never hidden."
          >
            No commit named
          </p>
          <CodingSessionGateRows
            leadFor={leadFor(true)}
            rows={gate.unnamedRows}
            testId="coding-session-landing-gate-unnamed-rows"
          />
        </div>
      ) : null}
    </>
  );
}

/**
 * Running lines keyed by gate name: those whose gate has a named row go
 * above its first named row; else above its first commitless row; the rest
 * stand alone above every row.
 */
export function groupRunningLines(
  gate: Pick<CodingSessionLandingGateRow, "rows" | "unnamedRows" | "running">,
): {
  byGate: ReadonlyMap<string, readonly CodingSessionLandingRunningLine[]>;
  alone: readonly CodingSessionLandingRunningLine[];
} {
  const withRow = new Set([
    ...gate.rows.map((row) => row.gate),
    ...gate.unnamedRows.map((row) => row.gate),
  ]);
  const byGate = new Map<string, CodingSessionLandingRunningLine[]>();
  const alone: CodingSessionLandingRunningLine[] = [];
  for (const line of gate.running) {
    if (!withRow.has(line.gate)) {
      alone.push(line);
      continue;
    }
    const list = byGate.get(line.gate) ?? [];
    list.push(line);
    byGate.set(line.gate, list);
  }
  return { byGate, alone };
}

function RunningLine({ line }: { line: CodingSessionLandingRunningLine }) {
  return (
    <li
      className={cn(
        "flex items-center gap-1.5 text-2xs",
        line.stale ? "text-muted-foreground" : "text-foreground",
      )}
      data-gate={line.gate}
      data-gate-run-state={line.stale ? "no-result" : "running"}
      data-testid={
        line.stale
          ? "coding-session-gate-start-stale"
          : "coding-session-gate-running"
      }
      title={line.title}
    >
      {line.stale ? (
        <CircleHelp aria-hidden className="size-3 shrink-0" />
      ) : (
        <LoaderCircle
          aria-hidden
          className="size-3 shrink-0 animate-spin text-primary"
        />
      )}
      <span className="min-w-0 truncate">{line.line}</span>
      <CodingSessionGateStartEventButton eventId={line.eventId} />
    </li>
  );
}

/**
 * A start event's id, copyable: "find it on the wire". The same affordance
 * and test id as the Audit tab's gate-start rows.
 */
export function CodingSessionGateStartEventButton({
  eventId,
}: {
  eventId: string;
}) {
  const [copied, setCopied] = React.useState(false);
  return (
    <button
      aria-label={`Copy start event id ${eventId}`}
      className="shrink-0 rounded-sm font-mono text-muted-foreground underline-offset-2 hover:underline focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
      data-event-id={eventId}
      data-testid="coding-session-gate-start-event"
      onClick={() => {
        void navigator.clipboard?.writeText(eventId).then(
          () => setCopied(true),
          () => setCopied(false),
        );
      }}
      title={`The start event on the wire: ${eventId}. Click to copy.`}
      type="button"
    >
      {copied ? "copied" : `start ${eventId.slice(0, 8)}`}
    </button>
  );
}
