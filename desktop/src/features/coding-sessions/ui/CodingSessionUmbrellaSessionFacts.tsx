import * as React from "react";

import type { CodingSessionExecution } from "@/features/coding-sessions/lib/codingSessionTypes";
import type { CodingSessionUmbrellaParticipant } from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";

import {
  type CodingSessionComposerSandbox,
  CodingSessionComposerSandboxChip,
} from "./CodingSessionComposerSandboxChip";
import {
  CodingSessionSeatSandboxTag,
  type CodingSessionSeatSandboxWarning,
  codingSessionSeatSandboxWarning,
} from "./CodingSessionComposerSeatSandbox";
import {
  type CodingSessionContinuityRow,
  codingSessionContinuityRows,
} from "./CodingSessionHeaderDetailsContinuity";
import {
  codingSessionExecutionSandbox,
  codingSessionGenerationTranscripts,
  codingSessionUmbrellaGenerationFacts,
} from "./codingSessionUmbrellaExecutionSandbox";

export {
  type CodingSessionGenerationFacts,
  codingSessionExecutionSandbox,
  codingSessionUmbrellaGenerationFacts,
} from "./codingSessionUmbrellaExecutionSandbox";

/**
 * Session facts for the umbrella (Mission) workspace (SV-16, SV-17).
 *
 * Continuity and boundary rows left the transcript (decision D3). The single
 * workspace feeds them to its header's Details and to the composer chip; the
 * umbrella workspace renders its own header and, when closed, no composer, so
 * without this module its Details showed no continuity at all and a closed
 * Mission named only the focused seat's boundary.
 */

/**
 * The focused execution's continuity rows, newest first, across every
 * generation the umbrella renders — a resume adds a generation, and the
 * start that began it published its own continuity status.
 */
export function codingSessionUmbrellaContinuityRows(
  execution: CodingSessionExecution,
): CodingSessionContinuityRow[] {
  return codingSessionContinuityRows(
    codingSessionUmbrellaGenerationFacts(
      codingSessionGenerationTranscripts(execution),
      null,
    ).facts,
  );
}

/**
 * {@link codingSessionUmbrellaContinuityRows} for a live execution. Keyed on
 * each generation's transcript reference rather than on the execution (a new
 * object per streamed item): a streamed item costs one classifier pass over
 * the running generation, prior generations are not re-read, and the rows —
 * the Details provider's value — keep their reference until a fact arrives.
 */
export function useCodingSessionUmbrellaContinuity(
  execution: CodingSessionExecution,
): readonly CodingSessionContinuityRow[] {
  const previousRef = React.useRef<ReturnType<
    typeof codingSessionUmbrellaGenerationFacts
  > | null>(null);
  const next = codingSessionUmbrellaGenerationFacts(
    codingSessionGenerationTranscripts(execution),
    previousRef.current,
  );
  // Committed, never written during render (SV-45, as in
  // `useCodingSessionExecutionSandboxes`).
  React.useLayoutEffect(() => {
    previousRef.current = next;
  });
  const { facts } = next;
  return React.useMemo(() => codingSessionContinuityRows(facts), [facts]);
}

/** A seat other than the focused one that ran outside a boundary. */
export type CodingSessionUmbrellaOtherSeatWarning = {
  executionKey: string;
  seatLabel: string;
  warning: CodingSessionSeatSandboxWarning;
};

/**
 * Every execution seat except the focused one that runs, or ran in any
 * generation, outside a boundary — in participant order. The focused seat's
 * boundary is already the footer's chip.
 */
export function codingSessionUmbrellaOtherSeatWarnings(
  participants: readonly CodingSessionUmbrellaParticipant[],
  focusedExecutionKey: string,
): CodingSessionUmbrellaOtherSeatWarning[] {
  const warnings: CodingSessionUmbrellaOtherSeatWarning[] = [];
  for (const participant of participants) {
    if (participant.kind !== "execution") continue;
    if (participant.executionKey === focusedExecutionKey) continue;
    const warning = codingSessionSeatSandboxWarning(
      codingSessionExecutionSandbox(participant.execution),
    );
    if (!warning) continue;
    warnings.push({
      executionKey: participant.executionKey,
      seatLabel: participant.label,
      warning,
    });
  }
  return warnings;
}

/**
 * The closed umbrella's footer: the focused seat's boundary chip, as the
 * single workspace's closed footer draws it, followed by a named tag for
 * every other seat that ran outside a boundary. Every seat is read across
 * all its generations. One row; when the tags do not fit they scroll rather
 * than being cut, so no unsandboxed seat is hidden.
 *
 * It is mounted only while the session is closed, so the open Mission does
 * no boundary reads for it on each streamed item.
 */
export function CodingSessionUmbrellaClosedSandboxFooter({
  focusedExecution,
  participants,
}: {
  focusedExecution: CodingSessionExecution;
  participants: readonly CodingSessionUmbrellaParticipant[];
}) {
  const focusedExecutionKey = focusedExecution.executionKey;
  const sandbox = React.useMemo<CodingSessionComposerSandbox>(
    () => ({
      report: codingSessionExecutionSandbox(focusedExecution),
      local: null,
    }),
    [focusedExecution],
  );
  const otherSeats = React.useMemo(
    () =>
      codingSessionUmbrellaOtherSeatWarnings(participants, focusedExecutionKey),
    [focusedExecutionKey, participants],
  );
  const focusedSeatLabel =
    participants.find(
      (participant) =>
        participant.kind === "execution" &&
        participant.executionKey === focusedExecutionKey,
    )?.label ?? null;
  return (
    <div
      className="flex h-10 shrink-0 items-center gap-2 border-t border-border/50 px-4 text-xs text-muted-foreground"
      data-testid="coding-session-sandbox-footer"
    >
      {focusedSeatLabel && otherSeats.length > 0 ? (
        <span className="shrink-0">{focusedSeatLabel}</span>
      ) : null}
      <CodingSessionComposerSandboxChip
        readOnlyNote="This session is closed. This is the boundary its agent last reported; there is nothing here to change."
        sandbox={sandbox}
      />
      {otherSeats.length > 0 ? (
        <ul
          aria-label="Other seats outside a boundary"
          className="flex min-w-0 shrink items-center gap-2 overflow-x-auto"
          data-testid="coding-session-closed-other-seats-sandbox"
        >
          {otherSeats.map((seat) => (
            <li
              className="inline-flex shrink-0 items-center gap-1"
              data-execution-key={seat.executionKey}
              key={seat.executionKey}
            >
              <span>{seat.seatLabel}</span>
              <CodingSessionSeatSandboxTag warning={seat.warning} />
            </li>
          ))}
        </ul>
      ) : null}
      <span className="shrink-0">Session closed</span>
    </div>
  );
}
