import * as React from "react";

import {
  listenSessionPreview,
  type SessionPreviewDrivingEvent,
  type SessionPreviewState,
  sessionPreviewStatus,
  toSessionPreviewError,
} from "@/shared/api/tauriSessionPreview";

export type SessionPreviewStateRead = {
  /** Rust's newest state for this channel, or null before the first answer. */
  state: SessionPreviewState | null;
  /** The status read failed: the sentence, never a guessed state. */
  error: string | null;
  /** The last `session-preview://driving` event, or null. */
  drivingEvent: SessionPreviewDrivingEvent | null;
  /** Apply a command's returned state (commands answer with `PreviewState`). */
  apply: (state: SessionPreviewState) => void;
};

/**
 * One channel's preview state: the status read, then every
 * `session-preview://state` event for this channel. React state only, so it
 * dies with the view and needs no community reset.
 */
export function useSessionPreviewState(
  channelId: string | null,
): SessionPreviewStateRead {
  const [state, setState] = React.useState<SessionPreviewState | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [drivingEvent, setDrivingEvent] =
    React.useState<SessionPreviewDrivingEvent | null>(null);

  React.useEffect(() => {
    setState(null);
    setError(null);
    setDrivingEvent(null);
    if (!channelId) return;
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    void listenSessionPreview(channelId, {
      onState: (next) => {
        if (!cancelled) setState(next);
      },
      onDriving: (event) => {
        if (!cancelled) setDrivingEvent(event);
      },
    })
      .then((stop) => {
        if (cancelled) stop();
        else unlisten = stop;
      })
      .catch(() => {
        // No event bus (a plain browser build): the status read still answers.
      });
    sessionPreviewStatus(channelId)
      .then((next) => {
        if (!cancelled) setState((current) => current ?? next);
      })
      .catch((reason) => {
        if (!cancelled) setError(toSessionPreviewError(reason).message);
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [channelId]);

  const apply = React.useCallback(
    (next: SessionPreviewState) => {
      if (next.channelId === channelId) {
        setState(next);
        setError(null);
      }
    },
    [channelId],
  );

  return { state, error, drivingEvent, apply };
}
