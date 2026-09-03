import {
  codingSessionSeatPackLine,
  type PackRef,
} from "@/features/coding-sessions/lib/codingSessionPackRef";

/**
 * One line naming the persona pack a seat actually staged.
 *
 * Presentational only — every word comes from
 * {@link codingSessionSeatPackLine}. Unlike its sibling
 * {@link import("./CodingSessionSeatBeeLine").CodingSessionSeatBeeLine}, this
 * line always renders: `no pack staged` is itself a fact a founder needs to
 * see (no 30624 source, or an older host), not a silence to render past.
 *
 * The chip is narrow, so the line truncates; `title` keeps the full reading
 * (path included, when a pack was staged) reachable without opening anything.
 */
export function CodingSessionSeatPackLine({
  packRef,
}: {
  /** The seat's staged pack, or `null` when the wire carried none. */
  packRef: PackRef | null;
}) {
  const line = codingSessionSeatPackLine(packRef);
  return (
    <span
      className="block truncate text-2xs text-muted-foreground"
      data-testid="coding-session-seat-pack"
      title={packRef === null ? line : `${line} · ${packRef.path}`}
    >
      {line}
    </span>
  );
}
