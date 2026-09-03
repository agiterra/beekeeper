import * as React from "react";
import { Check, Minus, X } from "lucide-react";

import {
  CODING_SESSION_GATE_SUMMARY_LINE_LIMIT,
  CODING_SESSION_OBSERVATION_EMPTY,
  CODING_SESSION_OBSERVATION_NOT_REPORTED,
  type CodingSessionObservationGateView,
} from "@/features/coding-sessions/lib/codingSessionObservationView";
import { cn } from "@/shared/lib/cn";

/**
 * A signed gate row: what ran, what it said, and the command that said it.
 *
 * **One shared source.** The Audit tab's `Gate rows` section and the
 * Inspector's `Structured tests` card render this same component over the same
 * fold, so the two surfaces cannot disagree about whether a gate passed —
 * which is the whole point of live-run finding 26, where a seat's prose and a
 * verifier's reproduction differed and nothing on the wire could say which was
 * right.
 *
 * Three rules the row keeps:
 *
 * * **The command is verbatim.** Never shortened, never re-worded. A reader
 *   who copies it runs what the author ran.
 * * **The outcome is a word.** `passed` / `failed` / `not-run` — the same
 *   three 44244's `report.tests[].outcome` uses. Colour never carries it
 *   alone (§8 I9), so the word and an icon glyph go with every tint.
 * * **Provenance is shown, and never merged.** `observed` says a mechanism
 *   watched the command run; `declared` says its author reported it. Both
 *   appear; neither replaces the other.
 */
export function CodingSessionGateRows({
  emptyCopy = CODING_SESSION_OBSERVATION_EMPTY.gates,
  rows,
  testId,
}: {
  emptyCopy?: React.ReactNode;
  rows: readonly CodingSessionObservationGateView[];
  testId?: string;
}) {
  if (rows.length === 0) {
    return (
      <p className="text-xs text-muted-foreground" data-testid={testId}>
        {emptyCopy}
      </p>
    );
  }
  return (
    <ul
      aria-label="Signed gate rows"
      className="space-y-2"
      data-testid={testId}
    >
      {rows.map((row) => (
        <CodingSessionGateRow key={row.key} row={row} />
      ))}
    </ul>
  );
}

function CodingSessionGateRow({
  row,
}: {
  row: CodingSessionObservationGateView;
}) {
  const [showAll, setShowAll] = React.useState(false);
  const folded = Math.max(
    0,
    row.summaryLines.length - CODING_SESSION_GATE_SUMMARY_LINE_LIMIT,
  );
  const visible = showAll
    ? row.summaryLines
    : row.summaryLines.slice(0, CODING_SESSION_GATE_SUMMARY_LINE_LIMIT);
  return (
    <li
      className="rounded-lg border border-border/60 bg-muted/15 p-2.5"
      data-outcome={row.outcome}
      data-source={row.source}
      data-testid="coding-session-gate-row"
    >
      <div className="flex items-baseline justify-between gap-2">
        <p className="min-w-0 truncate text-xs font-medium">{row.gate}</p>
        <GateOutcome outcome={row.outcome} />
      </div>
      <code className="mt-1 block break-all font-mono text-2xs text-muted-foreground">
        {row.command}
      </code>
      {row.summaryLines.length === 0 ? (
        <p className="mt-1 text-2xs text-muted-foreground">
          No summary published.
        </p>
      ) : (
        <>
          <pre
            className="mt-1 max-h-64 overflow-x-auto rounded-sm bg-muted/40 p-1.5 font-mono text-2xs whitespace-pre-wrap"
            data-testid="coding-session-gate-row-summary"
          >
            {visible.join("\n")}
          </pre>
          {folded > 0 ? (
            <button
              className="mt-1 rounded-sm text-2xs font-medium text-primary underline-offset-2 hover:underline focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
              data-testid="coding-session-gate-row-show-all"
              onClick={() => setShowAll((current) => !current)}
              type="button"
            >
              {showAll
                ? "Show less"
                : `Show all (${folded} more line${folded === 1 ? "" : "s"})`}
            </button>
          ) : null}
          {row.hiddenSummaryLines > 0 ? (
            <p className="mt-1 text-2xs text-muted-foreground">
              {row.hiddenSummaryLines} further line
              {row.hiddenSummaryLines === 1 ? " is" : "s are"} on the wire and
              not shown here.
            </p>
          ) : null}
        </>
      )}
      <p className="mt-1 text-2xs text-muted-foreground">
        <GateSource source={row.source} />
        {" · "}
        {row.duration === null ? (
          <span title="The author published no duration for this gate.">
            {CODING_SESSION_OBSERVATION_NOT_REPORTED}
          </span>
        ) : (
          <span title="The author's own measurement.">
            {row.duration} · the author's own measurement
          </span>
        )}
      </p>
      {row.droppedEventIds > 0 ? (
        <p className="mt-1 text-2xs text-muted-foreground">
          {row.droppedEventIds} older statement
          {row.droppedEventIds === 1 ? "" : "s"} about this gate are on the wire
          and not listed here.
        </p>
      ) : null}
    </li>
  );
}

/**
 * The outcome word, with a glyph and a tint — the word first.
 *
 * §8 I9: colour never carries a state on its own, so the tint is the third
 * carrier, after the word and the mark.
 */
function GateOutcome({
  outcome,
}: {
  outcome: CodingSessionObservationGateView["outcome"];
}) {
  const mark = outcome === "passed" ? "✓" : outcome === "failed" ? "✕" : "–";
  return (
    <span
      className={cn(
        "shrink-0 rounded-md border px-1.5 py-0.5 text-2xs font-medium",
        outcome === "failed"
          ? "border-destructive/40 bg-destructive/10 text-destructive"
          : outcome === "passed"
            ? "border-border bg-muted/40 text-foreground"
            : "border-border/60 bg-transparent text-muted-foreground",
      )}
      data-testid="coding-session-gate-outcome"
    >
      <span aria-hidden>{mark} </span>
      {outcome}
    </span>
  );
}

/**
 * Where the row came from, in the wire's own word.
 *
 * `observed` is not a stronger version of `declared`; it is a different
 * author. Saying which is the only way a reader can tell a measurement from a
 * claim — the distinction the 2026-09-02 ruling exists to keep.
 */
function GateSource({
  source,
}: {
  source: CodingSessionObservationGateView["source"];
}) {
  return source === "observed" ? (
    <span title="Written by the mechanism that watched the command run, not by its subject.">
      observed
    </span>
  ) : (
    <span title="Reported by its own author. A claim, not a measurement.">
      declared
    </span>
  );
}

/**
 * The `passed` / `failed` / `not-run` glyph, beside the word — never instead
 * of it.
 *
 * Lives here rather than in the Inspector because both surfaces that render an
 * outcome now import from this file, and because the Inspector was two lines
 * under the 1,000-line gate: the section that grew had to take its helpers
 * with it (§0.4 — split, never bump).
 */
export function CodingSessionTestIcon({
  outcome,
}: {
  outcome: "passed" | "failed" | "not-run";
}) {
  const Icon = outcome === "passed" ? Check : outcome === "failed" ? X : Minus;
  return (
    <Icon
      aria-label={outcome}
      className={cn(
        "mt-0.5 size-3.5 shrink-0",
        outcome === "passed" && "text-emerald-600 dark:text-emerald-400",
        outcome === "failed" && "text-destructive",
        outcome === "not-run" && "text-muted-foreground",
      )}
    />
  );
}
