import * as React from "react";

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import { CODING_SESSION_CONTINUITY_TITLE } from "@/features/coding-sessions/lib/codingSessionTranscriptItems";
import { isCodingSessionContinuityLoss } from "@/features/coding-sessions/lib/codingSessionTranscriptModelPredicates";
import { cn } from "@/shared/lib/cn";
import { formatItemTimestamp } from "@/shared/lib/datetime";

/**
 * Session continuity in Details (SV-16, decision D3).
 *
 * The provider publishes one continuity status per execution start — fresh,
 * rehydrated, resumed, loaded, or restarted without context. Those rows used
 * to sit in the transcript's reading order; the transcript now leaves them
 * out (lane A2), and they are read here from the same transcript items, so
 * nothing the provider said is lost — it moved one click away.
 *
 * A start that *lost* context is not merely detail: the Details trigger
 * carries a warning dot for it, so the fact that the agent no longer
 * remembers earlier turns stays visible without opening anything.
 */

/** One continuity disclosure, newest first in {@link codingSessionContinuityRows}. */
export type CodingSessionContinuityRow = {
  id: string;
  text: string;
  /** ISO timestamp of the status item, or empty when it carried none. */
  timestamp: string;
  /** This start began without the session's prior context. */
  lost: boolean;
};

type ContinuityTranscriptItem = {
  id: string;
  type: string;
  title?: string;
  text?: string;
  timestamp?: string;
};

/**
 * Did this start lose context? The transcript model's own classifier
 * (`isCodingSessionContinuityLoss`), so the row Details flags is exactly the
 * row the transcript keeps in its reading order: a restart without context,
 * a fresh start for any reason but "first execution", and any prose this
 * build does not recognise as routine — an unknown status is not presented
 * as safe.
 */
export function codingSessionContinuityLost(text: string): boolean {
  return isCodingSessionContinuityLoss({
    id: "",
    type: "lifecycle",
    renderClass: "status",
    title: CODING_SESSION_CONTINUITY_TITLE,
    text,
  } as TranscriptItem);
}

/** Every continuity status in a transcript, newest first. */
export function codingSessionContinuityRows(
  items: readonly ContinuityTranscriptItem[],
): CodingSessionContinuityRow[] {
  const rows: CodingSessionContinuityRow[] = [];
  for (const item of items) {
    if (
      item.type !== "lifecycle" ||
      item.title !== CODING_SESSION_CONTINUITY_TITLE ||
      !item.text
    ) {
      continue;
    }
    rows.push({
      id: item.id,
      text: item.text,
      timestamp: item.timestamp ?? "",
      lost: codingSessionContinuityLost(item.text),
    });
  }
  return rows.reverse();
}

/**
 * Carries the open session's continuity rows from the workspace (which has
 * the transcript) to the Details popover inside the header (which does not),
 * without threading a prop through the header. Absent — an empty list — is
 * "nothing published", and Details draws no section for it.
 */
const ContinuityContext = React.createContext<
  readonly CodingSessionContinuityRow[]
>([]);

export const CodingSessionDetailsContinuityProvider =
  ContinuityContext.Provider;

/** The continuity rows for the session whose header this is. */
export function useCodingSessionDetailsContinuity(): readonly CodingSessionContinuityRow[] {
  return React.useContext(ContinuityContext);
}

function rowTime(timestamp: string): string | null {
  const millis = Date.parse(timestamp);
  if (!Number.isFinite(millis)) return null;
  return formatItemTimestamp(millis / 1_000, { withTime: true });
}

/** The Details popover's "Session continuity" section. */
export function CodingSessionDetailsContinuity({
  rows,
}: {
  rows: readonly CodingSessionContinuityRow[];
}) {
  const [latest, ...earlier] = rows;
  if (!latest) return null;
  const latestTime = rowTime(latest.timestamp);
  return (
    <section
      className="mb-3 border-b border-border/60 pb-3"
      data-testid="coding-session-details-continuity"
    >
      <p className="text-xs text-muted-foreground">
        {CODING_SESSION_CONTINUITY_TITLE}
        {latestTime ? ` · ${latestTime}` : null}
      </p>
      <p
        className={cn(
          "mt-1 text-sm leading-snug",
          latest.lost && "text-amber-700 dark:text-amber-300",
        )}
        data-lost={latest.lost ? "true" : undefined}
        data-testid="coding-session-details-continuity-latest"
      >
        {latest.text}
      </p>
      {earlier.length > 0 ? (
        // Every earlier start, scrolled rather than truncated: these rows no
        // longer appear in the transcript, so this list is their only home.
        <ul className="mt-2 max-h-40 space-y-1 overflow-y-auto text-xs text-muted-foreground">
          {earlier.map((row) => {
            const time = rowTime(row.timestamp);
            return (
              <li
                className={cn(row.lost && "text-amber-700 dark:text-amber-300")}
                key={row.id}
              >
                {time ? <span className="tabular-nums">{time}: </span> : null}
                {row.text}
              </li>
            );
          })}
        </ul>
      ) : null}
    </section>
  );
}
