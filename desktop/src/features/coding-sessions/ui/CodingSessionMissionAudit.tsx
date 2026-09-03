import * as React from "react";

import {
  CODING_SESSION_AUDIT_USAGE_LIMIT,
  deriveCodingSessionMissionAudit,
  type CodingSessionMissionAuditSeatInput,
  type CodingSessionMissionAuditTotals,
  type CodingSessionMissionAuditTurn,
} from "@/features/coding-sessions/lib/codingSessionMissionAuditModel";
import type { CodingSessionObservationView } from "@/features/coding-sessions/lib/codingSessionObservationView";
import { cn } from "@/shared/lib/cn";
import { useElementWidth } from "@/shared/hooks/use-mobile";
import { CodingSessionObservationSections } from "./CodingSessionObservationSections";

export type CodingSessionMissionAuditProps = {
  /**
   * The seats' signed transcripts. The fold happens **here**, inside a
   * component the surface host mounts only while its own tab is selected, so a
   * live mission pays nothing for a rail nobody has opened (REVIEW-A3 F5).
   */
  seats: readonly CodingSessionMissionAuditSeatInput[];
  variant: "panel" | "drawer";
  loading?: boolean;
  errorMessage?: string | null;
  onRefresh?: () => void;
  /**
   * This session's folded kind-44246 observations — the **signed** half of
   * this rail. Everything below it is derived from transcript items the client
   * happens to hold; these four sections are records their authors signed.
   */
  observations: CodingSessionObservationView;
  /** True while the observation read is in flight. Unknown, never empty. */
  observationsLoading?: boolean;
  /** Why the observation read failed, or null. */
  observationsError?: string | null;
};

/**
 * The Audit rail tab: what this session spent, from its own signed items.
 *
 * Five sections in one reading order — what each turn cost, what the seats
 * cost together, and the three shapes of waste the 2026-09-01 run put on the
 * wire: the same file handed to a seat twice, the room downloaded to show one
 * row, and a command retried until the deadline with an identical answer every
 * time.
 *
 * Nothing here estimates. Every absent number is an em dash whose hover says
 * `not reported`, cost appears only where the driver priced the turn, and each
 * list carries its own bound in words when it bites.
 */
export function CodingSessionMissionAudit({
  errorMessage = null,
  loading = false,
  observations,
  observationsError = null,
  observationsLoading = false,
  onRefresh,
  seats,
  variant,
}: CodingSessionMissionAuditProps) {
  const audit = React.useMemo(
    () => deriveCodingSessionMissionAudit(seats),
    [seats],
  );
  const [sortBySeat, setSortBySeat] = React.useState(false);
  const turns = React.useMemo(
    () => (sortBySeat ? sortTurnsBySeat(audit.turns) : audit.turns),
    [audit.turns, sortBySeat],
  );
  return (
    <aside
      aria-label="Mission audit"
      className="flex h-full min-h-0 w-full flex-col bg-background text-foreground"
      data-testid="coding-session-mission-audit"
      data-variant={variant}
    >
      {loading ? (
        <p
          className="border-b border-border/60 bg-muted/20 px-4 py-2 text-xs text-muted-foreground"
          role="status"
        >
          Loading signed Mission evidence…
        </p>
      ) : null}
      {errorMessage ? (
        <div
          className="border-b border-destructive/35 bg-destructive/10 px-4 py-2"
          role="alert"
        >
          <p className="text-xs text-destructive">{errorMessage}</p>
          {onRefresh ? (
            <button
              className="mt-1 rounded-sm text-2xs font-medium text-primary underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              onClick={onRefresh}
              type="button"
            >
              Retry signed evidence
            </button>
          ) : null}
        </div>
      ) : null}

      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-4 pb-8">
        {/* The signed half first. Everything below it is this client's own
            arithmetic over transcript items; these are records with authors. */}
        <AuditSection title="Signed observations">
          <CodingSessionObservationSections
            errorMessage={observationsError}
            loading={observationsLoading}
            view={observations}
          />
        </AuditSection>

        <AuditSection
          title="Per turn"
          truncations={audit.truncations.filter(
            (entry) => entry.section === "turns",
          )}
          trailing={
            audit.turns.length > 0 ? (
              <button
                aria-pressed={sortBySeat}
                className={cn(
                  "rounded-md border px-1.5 py-0.5 text-2xs transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                  sortBySeat
                    ? "border-border bg-muted/50 text-foreground"
                    : "border-border/60 text-muted-foreground hover:text-foreground",
                )}
                data-testid="coding-session-mission-audit-sort"
                onClick={() => setSortBySeat((current) => !current)}
                type="button"
              >
                {sortBySeat ? "Grouped by seat" : "In time order"}
              </button>
            ) : null
          }
        >
          {audit.turns.length === 0 ? (
            <EmptyCopy>
              No signed turn has closed in this session yet.
              <Sentence>
                A turn appears here when its driver publishes the `result` item
                that ends it.
              </Sentence>
            </EmptyCopy>
          ) : (
            <PerTurnTable turns={turns} />
          )}
          {audit.turns.some((turn) => !turn.toolCallsReported) ? (
            <Sentence>
              * counted from the tool calls that turn published; the driver
              reported no count of its own.
            </Sentence>
          ) : null}
          {audit.turns.length > 0 && audit.reportedSeatCount === 0 ? (
            <Sentence>{CODING_SESSION_AUDIT_USAGE_LIMIT}</Sentence>
          ) : null}
        </AuditSection>

        <AuditSection title="Totals">
          {audit.totalsBySeat.length === 0 ? (
            <EmptyCopy>No signed session seats projected.</EmptyCopy>
          ) : (
            <div className="space-y-2">
              {audit.totalsBySeat.map((entry) => (
                <TotalsRow
                  key={entry.executionKey}
                  label={entry.seat}
                  totals={entry.totals}
                />
              ))}
              <TotalsRow label="Σ this session" totals={audit.sessionTotals} />
              {audit.sessionTotals.reportedTurns < audit.sessionTotals.turns ? (
                <Sentence>
                  * counted from the tool calls those turns published; their
                  drivers reported no count of their own.
                </Sentence>
              ) : null}
            </div>
          )}
        </AuditSection>

        <AuditSection
          title="Handed twice"
          truncations={audit.truncations.filter(
            (entry) => entry.section === "handed-twice",
          )}
        >
          {audit.handedTwice.length === 0 ? (
            <EmptyCopy>
              No seat asked for the same path or command twice.
            </EmptyCopy>
          ) : (
            <ul
              className="space-y-1.5"
              data-testid="mission-audit-handed-twice"
            >
              {audit.handedTwice.map((row) => (
                <li key={`${row.seat}:${row.what}:${row.key}`}>
                  <p className="flex items-baseline justify-between gap-2 text-xs">
                    <code className="min-w-0 truncate font-mono">
                      {row.key}
                    </code>
                    <span className="shrink-0 tabular-nums">
                      ×{row.count} ·{" "}
                      <Bytes clipped={row.bytesClipped} value={row.bytes} />
                    </span>
                  </p>
                  <p className="text-2xs text-muted-foreground">
                    {row.seat} · {row.what === "path" ? "path" : "command"}
                    {row.resultsSeen < row.count
                      ? ` · ${row.resultsSeen} of ${row.count} answers seen`
                      : null}
                  </p>
                </li>
              ))}
            </ul>
          )}
        </AuditSection>

        <AuditSection
          title="Downloads the room"
          truncations={audit.truncations.filter(
            (entry) => entry.section === "room-downloads",
          )}
        >
          {audit.roomDownloads.length === 0 ? (
            <EmptyCopy>No unbounded relay read observed.</EmptyCopy>
          ) : (
            <ul
              className="space-y-1.5"
              data-testid="mission-audit-room-downloads"
            >
              {audit.roomDownloads.map((row) => (
                <li key={`${row.seat}:${row.command}`}>
                  <p className="flex items-baseline justify-between gap-2 text-xs">
                    <code className="min-w-0 truncate font-mono">
                      {row.command}
                    </code>
                    <span className="shrink-0 tabular-nums">×{row.count}</span>
                  </p>
                  <p className="text-2xs text-muted-foreground">{row.seat}</p>
                </li>
              ))}
              <Sentence>
                These read the whole session to show one row. Narrow them with
                an explicit filter where the command has one.
              </Sentence>
            </ul>
          )}
        </AuditSection>

        <AuditSection
          title="Retry loops"
          truncations={audit.truncations.filter(
            (entry) => entry.section === "retry-loops",
          )}
        >
          {audit.retryLoops.length === 0 ? (
            <EmptyCopy>
              No command repeated back to back with an identical result.
            </EmptyCopy>
          ) : (
            <ul className="space-y-1.5" data-testid="mission-audit-retry-loops">
              {audit.retryLoops.map((row) => (
                <li key={`${row.seat}:${row.command}`}>
                  <p className="flex items-baseline justify-between gap-2 text-xs">
                    <code className="min-w-0 truncate font-mono">
                      {row.command}
                    </code>
                    <span className="shrink-0 tabular-nums">×{row.count}</span>
                  </p>
                  <p className="text-2xs text-muted-foreground">
                    {row.seat} ·{" "}
                    {row.identicalResults === true
                      ? "identical results"
                      : "results not published — agreement unknown"}
                  </p>
                </li>
              ))}
            </ul>
          )}
        </AuditSection>
      </div>
    </aside>
  );
}

/**
 * `(1 of 3 turns reported usage)` — SURFACES H5, per **turn**.
 *
 * Seat-granularity hid the case that matters: one turn of eight reporting made
 * the whole seat "reported", and its Σ printed a third of the work with
 * nothing saying so (REVIEW-A3 F2). Absent when every contributing turn
 * reported, and absent when there are no turns to describe.
 */
function partialDisclosure(
  totals: CodingSessionMissionAuditTotals,
): string | null {
  if (totals.turns === 0) return null;
  if (totals.reportedTurns === totals.turns) return null;
  return `(${totals.reportedTurns} of ${totals.turns} turn${
    totals.turns === 1 ? "" : "s"
  } reported usage)`;
}

/** Group turns under their seat while keeping each seat's own chronology. */
function sortTurnsBySeat(
  turns: readonly CodingSessionMissionAuditTurn[],
): readonly CodingSessionMissionAuditTurn[] {
  return [...turns].sort(
    (left, right) =>
      left.seat.localeCompare(right.seat) ||
      (left.startedAt ?? 0) - (right.startedAt ?? 0),
  );
}

/** Below this, eight columns cannot be read and the table becomes cards. */
const PER_TURN_TABLE_MIN_WIDTH_PX = 400;

function PerTurnTable({
  turns,
}: {
  turns: readonly CodingSessionMissionAuditTurn[];
}) {
  const [widthRef, widthPx] = useElementWidth<HTMLDivElement>();
  // B6/B7: eight columns in a 300 px rail meant a sideways scroller nested
  // inside the Audit tab's vertical scroller — the wheel doing two different
  // things over 40 px of pointer travel, and a horizontal flick that meant
  // nothing carrying cost and time out of view. Below the floor the same rows
  // render as one card per turn: every value still shown, one axis of scroll.
  // `widthPx === 0` is "not measured yet", not "narrow", so the table renders
  // until a measurement says otherwise.
  if (widthPx > 0 && widthPx < PER_TURN_TABLE_MIN_WIDTH_PX) {
    return (
      <div ref={widthRef}>
        <ul className="space-y-2" data-testid="mission-audit-per-turn-cards">
          {turns.map((turn) => (
            <li
              className="rounded-lg border border-border/50 p-2"
              data-execution={turn.executionKey}
              data-testid="mission-audit-turn-card"
              key={`${turn.executionKey}:${turn.turnId ?? "no-turn"}`}
            >
              <p className="truncate text-xs font-medium">{turn.seat}</p>
              <dl className="mt-1 grid grid-cols-2 gap-x-3 text-2xs tabular-nums">
                <PerTurnCardRow label="Started">
                  <Clock value={turn.startedAt} />
                </PerTurnCardRow>
                <PerTurnCardRow label="Duration">
                  <Duration value={turn.durationMs} />
                </PerTurnCardRow>
                <PerTurnCardRow label="Tools">
                  <ToolCalls turn={turn} />
                </PerTurnCardRow>
                <PerTurnCardRow label="Out">
                  <Num value={turn.outputTokens} />
                </PerTurnCardRow>
                <PerTurnCardRow label="Cache reads">
                  <Num value={turn.cacheReadTokens} />
                </PerTurnCardRow>
                <PerTurnCardRow label="Cache writes">
                  <Num value={turn.cacheWriteTokens} />
                </PerTurnCardRow>
                <PerTurnCardRow label="Context window">
                  <Num value={turn.contextWindow} />
                </PerTurnCardRow>
              </dl>
            </li>
          ))}
        </ul>
      </div>
    );
  }
  return (
    <div className="-mx-1 overflow-x-auto px-1" ref={widthRef}>
      <table
        className="w-full min-w-max border-collapse text-2xs tabular-nums"
        data-testid="mission-audit-per-turn"
      >
        <thead>
          <tr className="text-left text-muted-foreground">
            <th className="pr-3 pb-1 font-medium">Seat</th>
            <th className="pr-3 pb-1 font-medium">Started</th>
            <th className="pr-3 pb-1 font-medium">Duration</th>
            <th className="pr-3 pb-1 font-medium">Tools</th>
            <th className="pr-3 pb-1 font-medium">Out</th>
            <th className="pr-3 pb-1 font-medium">Cache reads</th>
            <th className="pr-3 pb-1 font-medium">Cache writes</th>
            <th className="pb-1 font-medium">Context window</th>
          </tr>
        </thead>
        <tbody>
          {turns.map((turn) => (
            <tr
              className="border-t border-border/40"
              data-execution={turn.executionKey}
              data-testid="mission-audit-turn-row"
              key={`${turn.executionKey}:${turn.turnId ?? "no-turn"}`}
            >
              <th
                className="max-w-32 truncate py-1 pr-3 text-left font-medium"
                scope="row"
              >
                {turn.seat}
              </th>
              <td className="py-1 pr-3">
                <Clock value={turn.startedAt} />
              </td>
              <td className="py-1 pr-3">
                <Duration value={turn.durationMs} />
              </td>
              <td className="py-1 pr-3">
                <ToolCalls turn={turn} />
              </td>
              <td className="py-1 pr-3">
                <Num value={turn.outputTokens} />
              </td>
              <td className="py-1 pr-3">
                <Num value={turn.cacheReadTokens} />
              </td>
              <td className="py-1 pr-3">
                <Num value={turn.cacheWriteTokens} />
              </td>
              <td className="py-1">
                <Num value={turn.contextWindow} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function PerTurnCardRow({
  children,
  label,
}: {
  children: React.ReactNode;
  label: string;
}) {
  return (
    <div className="flex items-baseline justify-between gap-2">
      <dt className="text-muted-foreground">{label}</dt>
      <dd>{children}</dd>
    </div>
  );
}

function TotalsRow({
  label,
  totals,
}: {
  label: string;
  totals: CodingSessionMissionAuditTotals;
}) {
  const disclosure = partialDisclosure(totals);
  return (
    <div data-testid="mission-audit-totals-row">
      <p className="flex items-baseline justify-between gap-2 text-xs font-medium">
        <span className="min-w-0 truncate">{label}</span>
        <span className="shrink-0 text-muted-foreground tabular-nums">
          {totals.turns} turn{totals.turns === 1 ? "" : "s"}
        </span>
      </p>
      <dl className="mt-0.5 grid grid-cols-2 gap-x-3 text-2xs">
        <Total
          label="Tools"
          observed={!totals.toolCallsReported}
          truncated={totals.toolCallsTruncated}
          value={totals.toolCalls}
        />
        <Total label="Out" value={totals.outputTokens} />
        <Total label="Cache reads" value={totals.cacheReadTokens} />
        <Total label="Cache writes" value={totals.cacheWriteTokens} />
        <div className="flex items-baseline justify-between gap-2">
          <dt className="text-muted-foreground">Wall</dt>
          <dd className="tabular-nums">
            <Duration value={totals.durationMs} />
          </dd>
        </div>
        <div className="flex items-baseline justify-between gap-2">
          <dt className="text-muted-foreground">Cost</dt>
          <dd className="tabular-nums">
            <Cost value={totals.costUsd} />
          </dd>
        </div>
      </dl>
      {disclosure ? (
        <p
          className="mt-1 text-2xs text-muted-foreground"
          data-testid="mission-audit-partial-disclosure"
        >
          {disclosure}
        </p>
      ) : null}
    </div>
  );
}

function Total({
  label,
  observed = false,
  truncated = false,
  value,
}: {
  label: string;
  /** The `*` the per-turn cells carry, so a Σ over them is marked too. */
  observed?: boolean;
  /** A total that summed even one floor is itself a floor. */
  truncated?: boolean;
  value: number | null;
}) {
  return (
    <div className="flex items-baseline justify-between gap-2">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="tabular-nums">
        {observed && value !== null ? (
          <span
            data-observed="true"
            data-truncated={truncated ? "true" : undefined}
            title={
              truncated
                ? "At least this many: this total sums a turn counted from a transcript that was cut short."
                : "Includes turns counted from the tool calls they published; their drivers reported no count of their own."
            }
          >
            {truncated ? "≥ " : null}
            {value.toLocaleString()}*
          </span>
        ) : (
          <Num value={value} />
        )}
      </dd>
    </div>
  );
}

/**
 * The turn's tool count, marked when it is the published-item count rather
 * than the driver's own. Both are measurements of the same turn — the `*` and
 * its hover say which one the reader is looking at.
 */
function ToolCalls({ turn }: { turn: CodingSessionMissionAuditTurn }) {
  if (turn.toolCallsReported) return <>{turn.toolCalls.toLocaleString()}</>;
  return (
    <span
      data-observed="true"
      data-truncated={turn.toolCallsTruncated ? "true" : undefined}
      title={
        turn.toolCallsTruncated
          ? "At least this many: counted from a transcript that was cut short, and the driver reported no count of its own."
          : "Counted from the tool calls this turn published; the driver reported no count of its own."
      }
    >
      {turn.toolCallsTruncated ? "≥ " : null}
      {turn.toolCalls.toLocaleString()}*
    </span>
  );
}

/** The one place an absent number is rendered, so it is absent everywhere. */
function Num({ value }: { value: number | null }) {
  if (value === null) return <NotReported />;
  return <>{value.toLocaleString()}</>;
}

/**
 * Published result bytes. A clipped result reads `≥`, because the provider
 * stopped at 8 KiB and the tool's own output was larger — a floor, and marked
 * as one rather than presented as the size.
 */
function Bytes({
  clipped = false,
  value,
}: {
  clipped?: boolean;
  value: number | null;
}) {
  if (value === null) return <NotReported />;
  const size =
    value < 1_024
      ? `${value} B`
      : value < 1_024 * 1_024
        ? `${Math.round(value / 1_024)} KB`
        : `${(value / (1_024 * 1_024)).toFixed(1)} MB`;
  if (!clipped) return <>{size}</>;
  return (
    <span
      data-clipped="true"
      title="At least this much: the provider clipped at least one of these results at 8 KiB."
    >
      ≥ {size}
    </span>
  );
}

function Duration({ value }: { value: number | null }) {
  if (value === null) return <NotReported />;
  const seconds = Math.round(value / 1_000);
  if (seconds < 60) return <>{seconds}s</>;
  return (
    <>
      {Math.floor(seconds / 60)}m {seconds % 60}s
    </>
  );
}

function Clock({ value }: { value: number | null }) {
  if (value === null) return <NotReported />;
  return (
    <>
      {new Date(value).toLocaleTimeString(undefined, {
        hour: "numeric",
        minute: "2-digit",
      })}
    </>
  );
}

/** Cost only where a pricing identity signed one; blank, never zero. */
function Cost({ value }: { value: number | null }) {
  if (value === null) return <NotReported />;
  return <>${value.toFixed(2)}</>;
}

function NotReported() {
  return (
    <span data-testid="mission-audit-not-reported" title="not reported">
      <span aria-hidden>—</span>
      <span className="sr-only">not reported</span>
    </span>
  );
}

function AuditSection({
  children,
  title,
  trailing = null,
  truncations = [],
}: {
  children: React.ReactNode;
  title: string;
  trailing?: React.ReactNode;
  truncations?: readonly { id: string; notice: string }[];
}) {
  return (
    <section className="border-b border-border/50 py-4 last:border-b-0">
      <div className="mb-2 flex items-center justify-between gap-2">
        <h3 className="text-2xs font-semibold tracking-wide text-muted-foreground uppercase">
          {title}
        </h3>
        {trailing}
      </div>
      {children}
      {truncations.map((truncation) => (
        <p
          className="mt-2 text-2xs text-amber-700 dark:text-amber-300"
          key={truncation.id}
          role="status"
        >
          {truncation.notice}
        </p>
      ))}
    </section>
  );
}

function EmptyCopy({ children }: { children: React.ReactNode }) {
  return <p className="text-xs text-muted-foreground">{children}</p>;
}

function Sentence({ children }: { children: React.ReactNode }) {
  return (
    <span className="mt-1 block text-2xs text-muted-foreground">
      {children}
    </span>
  );
}
