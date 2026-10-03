import * as React from "react";

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import type { CodingSessionTaskModel } from "@/features/coding-sessions/lib/codingSessionTaskModel";

export function deriveCodingSessionActiveTaskModel({
  isWorking,
  model,
  transcript,
}: {
  isWorking: boolean;
  model: CodingSessionTaskModel | null;
  transcript: TranscriptItem[];
}): CodingSessionTaskModel | null {
  if (!isWorking || model?.state !== "active" || !model.turnId) return null;
  const latestTurnId = latestTranscriptTurnId(transcript);
  return latestTurnId === model.turnId ? model : null;
}

/**
 * Per-turn attachment state for the Tasks rail above the composer.
 *
 * `open` means *mounted*, not *expanded*. On a wide window the rail mounts for
 * every turn with an active plan and renders as one collapsed line
 * (`CodingSessionTaskRail` variant `dock`), so nothing opens over the
 * transcript on its own; `close` dismisses it for this turn and the header's
 * Plan control — shown only while the rail is not — brings it back. On a
 * narrow window there is no rail, and `open` is the plan sheet, opened only
 * from that header control.
 */
export function useCodingSessionTaskDock({
  isNarrow,
  isWorking,
  model,
  transcript,
}: {
  isNarrow: boolean;
  isWorking: boolean;
  model: CodingSessionTaskModel | null;
  transcript: TranscriptItem[];
}) {
  const activeModel = React.useMemo(
    () => deriveCodingSessionActiveTaskModel({ isWorking, model, transcript }),
    [isWorking, model, transcript],
  );
  const turnId = activeModel?.turnId ?? null;
  const [dismissedTurnId, setDismissedTurnId] = React.useState<string | null>(
    null,
  );
  const [narrowOpenTurnId, setNarrowOpenTurnId] = React.useState<string | null>(
    null,
  );
  const open =
    turnId !== null &&
    (isNarrow ? narrowOpenTurnId === turnId : dismissedTurnId !== turnId);

  const close = React.useCallback(() => {
    if (!turnId) return;
    setDismissedTurnId(turnId);
    setNarrowOpenTurnId(null);
  }, [turnId]);

  const show = React.useCallback(() => {
    if (!turnId) return;
    setDismissedTurnId(null);
    setNarrowOpenTurnId(turnId);
  }, [turnId]);

  const toggle = React.useCallback(() => {
    if (open) close();
    else show();
  }, [close, open, show]);

  return { activeModel, close, open, show, toggle };
}

function latestTranscriptTurnId(transcript: TranscriptItem[]): string | null {
  for (let index = transcript.length - 1; index >= 0; index -= 1) {
    const turnId = transcript[index]?.turnId;
    if (turnId) return turnId;
  }
  return null;
}
