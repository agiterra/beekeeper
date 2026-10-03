import { ArrowUp } from "lucide-react";

import { UMBRELLA_TIMELINE_WINDOW_TURNS } from "./CodingSessionUmbrellaTimelineWindow";

/**
 * What the control says is above the window. Both numbers are exact:
 * `hiddenEntryCount` is every row not rendered, of any kind and any seat;
 * `hiddenTurnCount` is the turns the window *counts*, which with a seat
 * focused are that seat's alone — so the focused wording leads with the rows
 * and names whose turns the second number is.
 */
export function umbrellaLoadEarlierCopy({
  hiddenEntryCount,
  hiddenTurnCount,
  turnsCountedFor = null,
}: {
  hiddenEntryCount: number;
  hiddenTurnCount: number;
  turnsCountedFor?: string | null;
}): { summary: string; action: string } {
  const rows = `${hiddenEntryCount} earlier ${hiddenEntryCount === 1 ? "row" : "rows"}`;
  const turns = `${hiddenTurnCount} ${hiddenTurnCount === 1 ? "turn" : "turns"}`;
  const nextCount = Math.min(hiddenTurnCount, UMBRELLA_TIMELINE_WINDOW_TURNS);
  if (turnsCountedFor !== null) {
    return {
      summary:
        hiddenTurnCount > 0
          ? `${rows} not shown, ${turns} from ${turnsCountedFor}`
          : `${rows} not shown, none from ${turnsCountedFor}`,
      action:
        hiddenTurnCount > 0
          ? `Load ${nextCount} earlier from ${turnsCountedFor}`
          : "Load earlier",
    };
  }
  return {
    summary:
      hiddenTurnCount > 0
        ? `${hiddenTurnCount} earlier ${hiddenTurnCount === 1 ? "turn" : "turns"} not shown (${hiddenEntryCount} ${hiddenEntryCount === 1 ? "row" : "rows"})`
        : `${rows} not shown`,
    action: hiddenTurnCount > 0 ? `Load ${nextCount} earlier` : "Load earlier",
  };
}

/**
 * The top of the umbrella timeline's render window: says how much is not
 * rendered, and renders the next `UMBRELLA_TIMELINE_WINDOW_TURNS` turns on
 * press. Nothing is withheld — the counts are the truth about what is above.
 */
export function CodingSessionUmbrellaLoadEarlier({
  hiddenEntryCount,
  hiddenTurnCount,
  onLoadEarlier,
  turnsCountedFor = null,
}: {
  hiddenEntryCount: number;
  hiddenTurnCount: number;
  onLoadEarlier: () => void;
  /** The focused seat's label when only its turns are counted. */
  turnsCountedFor?: string | null;
}) {
  const copy = umbrellaLoadEarlierCopy({
    hiddenEntryCount,
    hiddenTurnCount,
    turnsCountedFor,
  });
  return (
    <div
      className="flex items-center justify-center gap-2 text-xs text-muted-foreground"
      data-testid="coding-session-umbrella-load-earlier"
      role="status"
    >
      <span>{copy.summary}</span>
      <span aria-hidden>·</span>
      <button
        className="inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 font-medium text-foreground transition-colors hover:bg-muted/45 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        data-testid="coding-session-umbrella-load-earlier-button"
        onClick={onLoadEarlier}
        type="button"
      >
        <ArrowUp aria-hidden className="size-3.5" />
        {copy.action}
      </button>
    </div>
  );
}
