import * as React from "react";
import { Camera, Smartphone } from "lucide-react";

import {
  type SurfaceObserverTarget,
  useSurfaceObserver,
} from "@/features/coding-sessions/hooks/useSurfaceObserver";
import { deriveCodingSessionDeviceView } from "@/features/coding-sessions/lib/codingSessionDevice";
import {
  SURFACE_HOST_DID_NOT_ANSWER,
  surfaceMachineName,
} from "@/features/coding-sessions/lib/codingSessionSurfaceSnapshot";
import { cn } from "@/shared/lib/cn";
import { rewriteRelayUrl } from "@/shared/lib/mediaUrl";
import { Button } from "@/shared/ui/button";

import { CodingSessionSurfaceSnapshotCard } from "./CodingSessionSurfaceSnapshotCard";
import type { CodingSessionSurfaceCtx } from "./surfaces/codingSessionSurfaceContext";
import type { CodingSessionDeviceExtension } from "./surfaces/CodingSessionSurfaceDevice";

/** How many recent snapshot cards the panel lists. */
const RECENT_SNAPSHOTS = 5;

/**
 * The Device surface (SV-34): the session's simulator as its provider
 * reports it. One header (`iPhone 17 · iOS 27.0 · on <machine>`), exactly one
 * status, the live frame while it is fresh (dimmed and never "Live" once it
 * goes stale), else the newest snapshot, a Snapshot now button, and recent
 * snapshot cards. Every word comes from the provider's signed records or the
 * frame stream; "no devices" is never said for a provider that offers none.
 */
export function CodingSessionDeviceSurface({
  ctx,
  extension,
}: {
  ctx: CodingSessionSurfaceCtx;
  extension: CodingSessionDeviceExtension;
}) {
  const { resolveActorName } = ctx;
  const machine = surfaceMachineName({
    providerPubkey: extension.providerPubkey,
    localProviderPubkey: extension.localProviderPubkey,
    nameOf: resolveActorName,
  });
  const base = deriveCodingSessionDeviceView({
    fold: extension.fold,
    targetKey: extension.targetKey,
    machine,
    observer: null,
    now: Date.now(),
    nameOf: resolveActorName,
  });
  const watchSlot = base.watch?.slot ?? null;
  const watchProducer = base.watch?.producerPubkey ?? null;
  const target = React.useMemo<SurfaceObserverTarget | null>(
    () =>
      watchSlot && watchProducer && ctx.channelId
        ? {
            channelId: ctx.channelId,
            surface: "device",
            key: watchSlot,
            producerPubkey: watchProducer,
            viewerPubkey: ctx.currentUserPubkey,
          }
        : null,
    [ctx.channelId, ctx.currentUserPubkey, watchProducer, watchSlot],
  );
  const observer = useSurfaceObserver(target);
  const view = deriveCodingSessionDeviceView({
    fold: extension.fold,
    targetKey: extension.targetKey,
    machine,
    observer: {
      status: observer.status,
      frameAt: observer.lastFrameAt,
      cadenceMs: observer.cadenceMs,
      actor: observer.frame?.actor ?? null,
    },
    now: observer.now,
    nameOf: resolveActorName,
  });

  const loading = extension.isLoading;
  const failed = !loading && extension.errorMessage !== null;
  const statusState = loading ? "loading" : failed ? "error" : view.status;
  const statusText = loading
    ? "Reading this session's device records…"
    : failed
      ? `Could not read device records: ${extension.errorMessage}`
      : view.statusText;
  const frame = view.showFrame ? observer.frame : null;
  const snapshot = view.newestSnapshot;

  return (
    <div
      className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-3"
      data-available="true"
      data-testid="coding-session-surface-panel-device"
    >
      <div className="flex items-start gap-2">
        <Smartphone
          aria-hidden
          className="mt-0.5 size-4 shrink-0 text-muted-foreground"
        />
        <div className="min-w-0 flex-1 space-y-0.5">
          <p
            className="truncate text-sm font-medium"
            data-testid="device-surface-header"
          >
            {view.header}
          </p>
          <p
            className={cn(
              "text-xs",
              view.status === "live" && !loading
                ? "text-foreground"
                : "text-muted-foreground",
            )}
            data-state={statusState}
            data-testid="device-surface-status"
            role="status"
          >
            {statusText}
          </p>
          {!loading && !failed && view.detail ? (
            <p
              className="text-2xs text-muted-foreground"
              data-testid="device-surface-detail"
            >
              {view.detail}
            </p>
          ) : null}
        </div>
      </div>

      {!loading && view.agentDeviceNote ? (
        <p
          className="rounded-md border border-border/60 bg-muted/30 px-2 py-1 text-2xs text-muted-foreground"
          data-testid="device-surface-agent-device-note"
        >
          {view.agentDeviceNote}
        </p>
      ) : null}

      {frame?.dataUrl ? (
        <img
          alt={`${view.header}, ${view.statusText}`}
          className={cn(
            "max-h-[60vh] w-full rounded-md border border-border/60 object-contain",
            view.status === "stalled" && "opacity-50 grayscale",
          )}
          data-state={view.status}
          data-testid="device-surface-frame"
          src={frame.dataUrl}
        />
      ) : snapshot ? (
        <img
          alt={snapshot.alt || view.statusText}
          className="max-h-[60vh] w-full rounded-md border border-border/60 object-contain opacity-90"
          data-snapshot-id={snapshot.id}
          data-testid="device-surface-snapshot"
          src={rewriteRelayUrl(snapshot.url)}
        />
      ) : null}

      {view.watch ? (
        <div className="flex flex-wrap items-center gap-2">
          <Button
            data-testid="device-surface-snapshot-now"
            disabled={observer.snapshotPending}
            onClick={() => void observer.requestSnapshot()}
            size="xs"
            variant="outline"
          >
            <Camera aria-hidden className="size-3" />
            {observer.snapshotPending
              ? "Asking for a snapshot…"
              : "Snapshot now"}
          </Button>
          {observer.snapshotTimedOut ? (
            <span
              className="text-2xs text-muted-foreground"
              data-testid="device-surface-unanswered"
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

      {view.snapshots.length > 0 ? (
        <section aria-label="Recent snapshots" className="space-y-1.5">
          <p className="text-3xs font-semibold tracking-wider text-muted-foreground uppercase">
            Recent snapshots
          </p>
          {view.snapshots.slice(0, RECENT_SNAPSHOTS).map((card) => (
            <CodingSessionSurfaceSnapshotCard
              card={card}
              key={card.id}
              localProviderPubkey={extension.localProviderPubkey}
              nameOf={resolveActorName}
            />
          ))}
        </section>
      ) : null}
    </div>
  );
}
