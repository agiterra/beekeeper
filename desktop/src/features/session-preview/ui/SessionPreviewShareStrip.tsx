import * as React from "react";
import { useQuery } from "@tanstack/react-query";
import { Camera, Radio } from "lucide-react";

import {
  listenSessionPreviewShare,
  type SessionPreviewShareState,
  sessionPreviewShareConfigure,
  sessionPreviewShareSnapshot,
  sessionPreviewShareState,
} from "@/shared/api/tauriSessionPreviewShare";
import { getCodingSessionProviderStatus } from "@/shared/api/tauriSessionProvider";
import { Button } from "@/shared/ui/button";
import { Switch } from "@/shared/ui/switch";

import {
  SESSION_PREVIEW_NO_SESSION_REF_SENTENCE,
  sessionPreviewShareStripText,
} from "../lib/previewShareModel";

function errorMessage(reason: unknown): string {
  if (reason && typeof reason === "object" && "message" in reason) {
    const message = (reason as { message?: unknown }).message;
    if (typeof message === "string" && message) return message;
  }
  return typeof reason === "string" ? reason : "The snapshot failed.";
}

/**
 * The host's honesty strip (SV-33 S3/S4): whether this computer's preview is
 * shared with the session and how many are watching, the Share toggle (on by
 * default, D5), and the camera button that publishes one snapshot. Every
 * word is Rust's last share state; with no sessionRef, nothing is shared and
 * the strip says why.
 */
export function SessionPreviewShareStrip({
  channelId,
  sessionRef,
}: {
  channelId: string;
  sessionRef: string | null;
}) {
  const [state, setState] = React.useState<SessionPreviewShareState | null>(
    null,
  );
  const [share, setShare] = React.useState<boolean | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [shooting, setShooting] = React.useState(false);
  const status = useQuery({
    queryKey: ["coding-session-provider-status"],
    queryFn: getCodingSessionProviderStatus,
    retry: false,
    staleTime: 60_000,
  });
  const statusSettled = status.data !== undefined || status.isError;
  const providerPubkey = status.data?.providerPubkey?.trim() || null;

  // The person's earlier choice for this session survives a remount.
  React.useEffect(() => {
    let cancelled = false;
    setShare(null);
    sessionPreviewShareState(channelId)
      .then((held: SessionPreviewShareState) => {
        if (cancelled) return;
        setState(held);
        setShare(held.sessionRef === sessionRef ? held.share : true);
      })
      .catch(() => {
        if (!cancelled) setShare(true);
      });
    return () => {
      cancelled = true;
    };
  }, [channelId, sessionRef]);

  React.useEffect(() => {
    if (share === null || !statusSettled) return;
    let cancelled = false;
    sessionPreviewShareConfigure({
      channelId,
      sessionRef,
      share,
      providerPubkey,
    })
      .then((next: SessionPreviewShareState) => {
        if (!cancelled) setState(next);
      })
      .catch((reason: unknown) => {
        if (!cancelled) setError(errorMessage(reason));
      });
    return () => {
      cancelled = true;
    };
  }, [channelId, providerPubkey, sessionRef, share, statusSettled]);

  React.useEffect(() => {
    let cancelled = false;
    let stop: (() => void) | null = null;
    void listenSessionPreviewShare(
      channelId,
      (next: SessionPreviewShareState) => {
        if (!cancelled) setState(next);
      },
    )
      .then((unlisten: () => void) => {
        if (cancelled) unlisten();
        else stop = unlisten;
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [channelId]);

  const unavailableSentence = sessionRef
    ? (state?.unavailable?.sentence ?? null)
    : (state?.unavailable?.sentence ?? SESSION_PREVIEW_NO_SESSION_REF_SENTENCE);
  const shareOn = state?.share ?? share ?? true;
  const text = sessionPreviewShareStripText({
    share: shareOn,
    watchers: state?.watchers.length ?? 0,
    unavailableSentence,
  });
  const cannotShare =
    !sessionRef || state?.unavailable?.code === "no_session_ref";

  return (
    <span
      className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1"
      data-share={shareOn ? "on" : "off"}
      data-unavailable={unavailableSentence ? "true" : "false"}
    >
      <span
        className="flex items-center gap-1"
        data-testid="session-preview-share-strip"
      >
        <Radio aria-hidden className="size-3" />
        {text}
      </span>
      <Switch
        aria-label="Share this preview with the session"
        checked={shareOn}
        className="scale-75"
        data-testid="session-preview-share-toggle"
        disabled={cannotShare || share === null}
        onCheckedChange={(checked) => {
          setError(null);
          setShare(checked);
        }}
      />
      <Button
        aria-label="Share a snapshot of this page"
        data-testid="session-preview-camera"
        // Share off publishes nothing, not even one snapshot.
        disabled={
          cannotShare || !shareOn || shooting || unavailableSentence !== null
        }
        onClick={() => {
          setShooting(true);
          setError(null);
          sessionPreviewShareSnapshot({ channelId })
            .catch((reason: unknown) => setError(errorMessage(reason)))
            .finally(() => setShooting(false));
        }}
        size="xs"
        title="Share a snapshot of this page"
        variant="ghost"
      >
        <Camera aria-hidden className="size-3" />
      </Button>
      {error ? (
        <span
          className="text-destructive"
          data-testid="session-preview-share-error"
          role="alert"
        >
          {error}
        </span>
      ) : null}
    </span>
  );
}
