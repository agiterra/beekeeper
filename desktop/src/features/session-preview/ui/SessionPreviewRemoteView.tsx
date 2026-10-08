import * as React from "react";
import { Camera, Globe } from "lucide-react";

import {
  type SurfaceObserverTarget,
  useSurfaceObserver,
  useSurfaceStoredEvents,
} from "@/features/coding-sessions/hooks/useSurfaceObserver";
import {
  compareSurfaceSnapshotsNewestFirst,
  deriveRemotePreviewView,
  parseSurfaceSnapshot,
  resolveSessionPreviewOwner,
  type SessionPreviewAnnounce,
  SURFACE_HOST_DID_NOT_ANSWER,
  type SurfaceNameOf,
  type SurfaceSnapshotCard,
} from "@/features/coding-sessions/lib/codingSessionSurfaceSnapshot";
import { CodingSessionSurfaceSnapshotCard } from "@/features/coding-sessions/ui/CodingSessionSurfaceSnapshotCard";
import {
  KIND_SESSION_PREVIEW_ANNOUNCE,
  KIND_SURFACE_SNAPSHOT,
} from "@/shared/constants/kinds";
import { cn } from "@/shared/lib/cn";
import { rewriteRelayUrl } from "@/shared/lib/mediaUrl";
import { Button } from "@/shared/ui/button";

/** How many recent snapshot cards the remote view lists. */
const RECENT_SNAPSHOTS = 5;

/** The session's preview as the relay has it: owner and snapshots. */
export type SessionPreviewRemoteRead = {
  /** The open 30626 that owns the session's preview, or null. */
  owner: SessionPreviewAnnounce | null;
  /** Preview 44253s for the session, newest first. */
  snapshots: readonly SurfaceSnapshotCard[];
  isLoading: boolean;
  errorMessage: string | null;
};

const PREVIEW_KINDS = [
  KIND_SESSION_PREVIEW_ANNOUNCE,
  KIND_SURFACE_SNAPSHOT,
] as const;

/**
 * Read the session's 30626 announces and preview 44253 snapshots (`#d` =
 * sessionRef), fold the owner (`resolve_preview_owner`), and keep both live.
 * React Query only; nothing at module level.
 */
export function useSessionPreviewRemoteRead(
  channelId: string | null,
  sessionRef: string | null,
): SessionPreviewRemoteRead {
  const dTags = React.useMemo(
    () => (sessionRef ? [sessionRef] : null),
    [sessionRef],
  );
  const read = useSurfaceStoredEvents({
    channelId,
    kinds: PREVIEW_KINDS,
    dTags,
    enabled: sessionRef !== null,
  });
  return React.useMemo(() => {
    if (!channelId || !sessionRef) {
      return {
        owner: null,
        snapshots: [],
        isLoading: false,
        errorMessage: null,
      };
    }
    const owner = resolveSessionPreviewOwner(
      read.events,
      channelId,
      sessionRef,
    );
    const snapshots = read.events
      .map(parseSurfaceSnapshot)
      .filter(
        (card): card is SurfaceSnapshotCard =>
          card !== null &&
          card.surface === "preview" &&
          card.key === sessionRef &&
          card.channelId === channelId,
      )
      .sort(compareSurfaceSnapshotsNewestFirst);
    return {
      owner,
      snapshots,
      isLoading: read.isLoading,
      errorMessage: read.errorMessage,
    };
  }, [channelId, read, sessionRef]);
}

/**
 * The Browser as seen from another computer (SV-33 S3/S4): frames from the
 * preview's owner while they are fresh, the newest snapshot otherwise, and a
 * Request snapshot button. A stale frame is dimmed and never labelled Live;
 * no open announce says so in a sentence rather than drawing an empty frame.
 */
export function SessionPreviewRemoteView({
  channelId,
  sessionRef,
  read,
  currentUserPubkey,
  localProviderPubkey,
  nameOf,
}: {
  channelId: string;
  sessionRef: string;
  read: SessionPreviewRemoteRead;
  currentUserPubkey: string | null;
  localProviderPubkey?: string | null;
  nameOf?: SurfaceNameOf;
}) {
  const ownerPubkey = read.owner?.signer ?? null;
  const target = React.useMemo<SurfaceObserverTarget | null>(
    () =>
      ownerPubkey
        ? {
            channelId,
            surface: "preview",
            key: sessionRef,
            producerPubkey: ownerPubkey,
            viewerPubkey: currentUserPubkey,
          }
        : null,
    [channelId, currentUserPubkey, ownerPubkey, sessionRef],
  );
  const observer = useSurfaceObserver(target);
  const newestSnapshot = read.snapshots[0] ?? null;
  const view = deriveRemotePreviewView({
    owner: read.owner,
    observer: {
      status: observer.status,
      frameAt: observer.lastFrameAt,
      cadenceMs: observer.cadenceMs,
      actor: observer.frame?.actor ?? null,
    },
    newestSnapshot,
    now: observer.now,
    nameOf,
  });
  const showFrame =
    observer.frame?.dataUrl &&
    (view.state === "live" ||
      view.state === "stalled" ||
      view.state === "paused");
  const loading = read.isLoading && !read.owner;

  return (
    <div
      className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-3"
      data-state={loading ? "loading" : view.state}
      data-testid="session-preview-remote"
    >
      <div className="flex items-start gap-2">
        <Globe
          aria-hidden
          className="mt-0.5 size-4 shrink-0 text-muted-foreground"
        />
        <div className="min-w-0 flex-1 space-y-0.5">
          {read.owner ? (
            <p className="truncate text-sm font-medium">
              {read.owner.title || read.owner.page}
            </p>
          ) : null}
          <p
            className={cn(
              "text-xs",
              view.state === "live"
                ? "text-foreground"
                : "text-muted-foreground",
            )}
            data-testid="session-preview-remote-status"
            role="status"
          >
            {loading
              ? "Reading this session's preview…"
              : read.errorMessage && !read.owner
                ? `Could not read this session's preview: ${read.errorMessage}`
                : view.text}
          </p>
          {!loading && view.hint ? (
            <p className="text-2xs text-muted-foreground">{view.hint}</p>
          ) : null}
        </div>
      </div>

      {showFrame && observer.frame?.dataUrl ? (
        <img
          alt={view.text}
          className={cn(
            "w-full rounded-md border border-border/60 object-contain",
            view.state !== "live" && "opacity-50 grayscale",
          )}
          data-state={view.state}
          data-testid="session-preview-remote-frame"
          src={observer.frame.dataUrl}
        />
      ) : newestSnapshot && view.state !== "none" ? (
        <img
          alt={newestSnapshot.alt || view.text}
          className="w-full rounded-md border border-border/60 object-contain opacity-90"
          data-snapshot-id={newestSnapshot.id}
          data-state="snapshot"
          data-testid="session-preview-remote-frame"
          src={rewriteRelayUrl(newestSnapshot.url)}
        />
      ) : null}

      {read.owner ? (
        <div className="flex flex-wrap items-center gap-2">
          <Button
            data-testid="session-preview-request-snapshot"
            disabled={observer.snapshotPending}
            onClick={() => void observer.requestSnapshot()}
            size="xs"
            variant="outline"
          >
            <Camera aria-hidden className="size-3" />
            {observer.snapshotPending
              ? "Asking for a snapshot…"
              : "Request snapshot"}
          </Button>
          {observer.snapshotTimedOut ? (
            <span
              className="text-2xs text-muted-foreground"
              data-testid="session-preview-remote-unanswered"
              role="status"
            >
              {SURFACE_HOST_DID_NOT_ANSWER}
            </span>
          ) : null}
          {observer.snapshotError ? (
            <span className="text-2xs text-destructive" role="alert">
              {observer.snapshotError}
            </span>
          ) : null}
        </div>
      ) : null}

      {read.snapshots.length > 0 ? (
        <section aria-label="Recent snapshots" className="space-y-1.5">
          <p className="text-3xs font-semibold tracking-wider text-muted-foreground uppercase">
            Recent snapshots
          </p>
          {read.snapshots.slice(0, RECENT_SNAPSHOTS).map((card) => (
            <CodingSessionSurfaceSnapshotCard
              card={card}
              key={card.id}
              localProviderPubkey={localProviderPubkey}
              nameOf={nameOf}
            />
          ))}
        </section>
      ) : null}
    </div>
  );
}
