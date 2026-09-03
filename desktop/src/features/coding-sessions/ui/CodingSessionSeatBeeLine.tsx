import {
  codingSessionSeatBeeLine,
  type SeatBeeStamp,
} from "@/features/coding-sessions/lib/codingSessionSeatBee";

/**
 * One line naming the `bee` a seat is actually running.
 *
 * Presentational only — every word comes from {@link codingSessionSeatBeeLine},
 * and the whole state is in the text, so a reader learns which build answered
 * without a colour, a tooltip, or a rail. A seat whose host published no
 * `beeStamp` renders nothing: absence of the key is not the same claim as an
 * unparsed `--version`, which reads `bee build unknown`.
 *
 * The chip is narrow, so the line truncates; `title` keeps the full reading
 * (path included) reachable without opening anything.
 */
export function CodingSessionSeatBeeLine({
  stamp,
}: {
  /** The seat's observed stamp, or `null` when the wire carried none. */
  stamp: SeatBeeStamp | null;
}) {
  const line = codingSessionSeatBeeLine(stamp);
  if (line === null) return null;
  return (
    <span
      className="block truncate text-2xs text-muted-foreground"
      data-testid="coding-session-seat-bee"
      title={line}
    >
      {line}
    </span>
  );
}
