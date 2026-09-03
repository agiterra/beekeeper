/**
 * One mission, rendered exactly as `buzz-core` wrote it.
 *
 * This file draws boxes. It does not decide what a mission means, in what
 * order its facts matter, or how any of them read as English — all of that is
 * folded and worded in Rust and arrives here as `{id, text}` lines. The `id` is
 * a closed token that says *where* a sentence belongs; the `text` is the
 * sentence, rendered verbatim.
 *
 * That split is the point of the lane. The CLI (`bee pulse digest`) and this
 * screen read the same fold, so a person switching between them sees the same
 * words about the same signed facts. A sentence composed here would drift from
 * the CLI's the first time either side was edited, and the surface would then
 * quietly describe the same events two different ways.
 *
 * The order of the four blocks is the model's order, not a design choice:
 * the mission's own lines (what it is waiting on, then what state it is in),
 * then per-seat truth (live now, gates, what is owed), then what actually
 * moved in git, then what it cost in time.
 */
import type {
  PulseLine,
  PulseMissionError,
  PulseMissionRow as PulseMissionRowModel,
  PulseMissionRowsResponse,
} from "../lib/pulseMissionWire";

/**
 * A list of model sentences, one test id per line id.
 *
 * Shared by every block on this surface so that a `gate` row and an `overlap`
 * row are addressable the same way — one convention, so a spec cannot assert
 * on a line family that quietly renamed itself.
 */
export function PulseMissionLineList<Id extends string>({
  className,
  lines,
  testId,
}: {
  className?: string;
  lines: readonly PulseLine<Id>[];
  testId?: string;
}) {
  if (lines.length === 0) return null;
  return (
    <div className={className ?? "flex flex-col gap-0.5"} data-testid={testId}>
      {lines.map((line) => (
        <p
          className="text-sm text-muted-foreground"
          data-testid={`pulse-mission-line-${line.id}`}
          // Keyed on the sentence, not the position: a mission carries two
          // `gate` lines, and a positional key would swap one seat's gate
          // result onto the other's row when the fold reorders them.
          key={`${line.id}:${line.text}`}
        >
          {line.text}
        </p>
      ))}
    </div>
  );
}

/**
 * A mission's own lines, minus the one that is about the reader.
 *
 * The model composes `not-read` on every row because a row is where it
 * composes lines; it is one statement about *the reader*, not about each
 * session, and `PulseRulingsWaitingCard` is the one place it belongs. Filtered
 * here rather than dropped in the model so the CLI still prints it and the two
 * consumers still read the same wire.
 */
function missionOwnLines<Id extends string>(
  lines: readonly PulseLine<Id>[],
): readonly PulseLine<Id>[] {
  return lines.filter((line) => line.id !== ("not-read" as Id));
}

/** The mission's title: the name it was given, else the ref it is known by. */
function missionTitle(mission: PulseMissionRowModel): string {
  return mission.name ?? mission.sessionRef ?? mission.sessionKey;
}

/** One mission row: its lines, its seats, what moved, and what it cost. */
export function PulseMissionRow({
  mission,
}: {
  mission: PulseMissionRowModel;
}) {
  return (
    <li
      className="rounded-md border border-border/60 bg-background/40 p-3"
      data-mission-state={mission.state}
      data-session-key={mission.sessionKey}
      data-testid={`pulse-mission-row-${mission.sessionKey}`}
    >
      <p
        className="truncate text-sm font-medium text-foreground"
        data-testid="pulse-mission-title"
      >
        {missionTitle(mission)}
      </p>

      <PulseMissionLineList
        className="mt-1.5 flex flex-col gap-0.5"
        lines={missionOwnLines(mission.lines)}
        testId="pulse-mission-lines"
      />

      {mission.seats.length > 0 ? (
        <ul
          className="mt-2 flex flex-col gap-1.5 border-l border-border/60 pl-2"
          data-testid="pulse-mission-seats"
        >
          {mission.seats.map((seat) => (
            <li
              data-seat-pubkey={seat.pubkey}
              data-seat-role={seat.role ?? ""}
              data-testid="pulse-mission-seat"
              key={seat.pubkey}
            >
              <PulseMissionLineList lines={seat.lines} />
            </li>
          ))}
        </ul>
      ) : null}

      {mission.moved.length > 0 ? (
        <ul
          className="mt-2 flex flex-col gap-1"
          data-testid="pulse-mission-moved-list"
        >
          {mission.moved.map((moved) => (
            <li
              data-moved-kind={moved.kind}
              data-moved-ref={moved.ref}
              data-testid="pulse-mission-moved"
              key={`${moved.kind}:${moved.sha}`}
            >
              <PulseMissionLineList lines={moved.lines} />
            </li>
          ))}
        </ul>
      ) : null}

      <PulseMissionLineList
        className="mt-2 flex flex-col gap-0.5 text-2xs"
        lines={mission.timing}
        testId="pulse-mission-timing"
      />
    </li>
  );
}

/**
 * The read's own disclosures: what it was scoped to, and what it lost.
 *
 * `missionErrors` renders unconditionally when non-empty, for the same reason
 * the digest's `errors[]` does: a session this read could not decode and a
 * project with nothing running produce the same empty list, and only this
 * block tells them apart. A read error must never render as a quiet project.
 */
export function PulseMissionsNotice({
  errors,
  scope,
  unreadable,
}: {
  errors: readonly PulseMissionError[];
  scope: string | null;
  /** A decode or transport failure, when this read produced no rows at all. */
  unreadable: string | null;
}) {
  if (!scope && errors.length === 0 && !unreadable) return null;
  return (
    <div className="flex flex-col gap-1" data-testid="pulse-missions-notice">
      {unreadable ? (
        <p
          className="rounded-md border border-destructive/40 bg-destructive/5 p-2 text-sm text-destructive"
          data-testid="pulse-missions-unreadable"
        >
          {unreadable}
        </p>
      ) : null}
      {scope ? (
        <p
          className="text-2xs text-muted-foreground"
          data-testid="pulse-missions-scope"
        >
          {scope}
        </p>
      ) : null}
      {errors.length > 0 ? (
        <ul
          className="list-disc pl-4 text-2xs text-muted-foreground"
          data-testid="pulse-mission-errors"
        >
          {errors.map((error) => (
            <li
              data-error-scope={error.scope}
              data-testid="pulse-mission-error"
              key={`${error.scope}:${error.message}`}
            >
              {error.message}
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}

/**
 * Every mission this read produced that no session card already carries.
 *
 * A mission row and a Pulse session card describe the same work from two
 * different reads, and they agree only when both saw it. `renderedSessionKeys`
 * is what the session cards already painted; everything else lands here, so a
 * mission the digest never saw is still on the screen rather than silently
 * dropped between two views of the same project.
 */
export function PulseMissionsSection({
  renderedSessionKeys,
  rows,
  unreadable,
}: {
  renderedSessionKeys?: ReadonlySet<string>;
  rows: PulseMissionRowsResponse | null;
  unreadable: string | null;
}) {
  const already = renderedSessionKeys ?? new Set<string>();
  const missions = (rows?.missions ?? []).filter(
    (mission) => !already.has(mission.sessionKey),
  );
  if (!rows && !unreadable) return null;
  return (
    <section className="flex flex-col gap-2" data-testid="pulse-missions">
      <PulseMissionsNotice
        errors={rows?.missionErrors ?? []}
        scope={rows?.missionScope ?? null}
        unreadable={unreadable}
      />
      {missions.length > 0 ? (
        <ul className="flex flex-col gap-2">
          {missions.map((mission) => (
            <PulseMissionRow key={mission.sessionKey} mission={mission} />
          ))}
        </ul>
      ) : null}
    </section>
  );
}
