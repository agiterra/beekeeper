import * as React from "react";

import {
  codingSessionDiffBaselineMarker,
  codingSessionDiffMarkerChanges,
  codingSessionDiffOpenedMarker,
  codingSessionNewestEditPerFile,
} from "@/features/coding-sessions/lib/codingSessionSurfaceBadgeModel";
import { codingSessionDiffBadgeFromCtx } from "@/features/coding-sessions/lib/codingSessionSurfaceBadgeModelCtx";
import {
  codingSessionSeenStorage,
  codingSessionSurfaceSeenKey,
  parseCodingSessionDiffSeenMarker,
  readCodingSessionSurfaceSeenRaw,
  subscribeCodingSessionSurfaceSeen,
  writeCodingSessionDiffSeenMarker,
} from "@/features/coding-sessions/lib/codingSessionSurfaceSeen";
import { CodingSessionSurfaceBadgePill } from "../CodingSessionSurfaceBadgePill";
import type { CodingSessionSurfaceCtx } from "./codingSessionSurfaceContext";
import type { CodingSessionSurfaceBadgeSlot } from "./codingSessionSurfaceRegistry";

/**
 * The Diff surface's badge (SV-22): files whose newest edit this device has
 * not shown, as a muted count — not live work, so never a live tone.
 *
 * This badge also keeps the marker it reads. While Diff is the active
 * surface it records every file's newest edit as seen; before Diff was ever
 * opened here it records every file's newest edit when this device first
 * showed the session, so old edits are not counted on first load (T3's rule
 * for its own badge) — by edit id, never by comparing clocks.
 */
export function CodingSessionSurfaceDiffBadge({
  ctx,
  slot,
}: {
  ctx: CodingSessionSurfaceCtx;
  slot: CodingSessionSurfaceBadgeSlot;
}) {
  const key = codingSessionSurfaceSeenKey({
    relayUrl: ctx.communityScope,
    channelId: ctx.channelId,
    sessionKey: ctx.sessionKey,
    surfaceId: "diff",
  });
  const raw = React.useSyncExternalStore(
    subscribeCodingSessionSurfaceSeen,
    () => readCodingSessionSurfaceSeenRaw(codingSessionSeenStorage(), key),
    () => null,
  );
  const marker = React.useMemo(
    () => parseCodingSessionDiffSeenMarker(raw),
    [raw],
  );
  const onScreen = ctx.activeSurfaceId === "diff";
  const transcript = ctx.transcript;
  const newest = React.useMemo(
    () => codingSessionNewestEditPerFile([...transcript]),
    [transcript],
  );

  React.useEffect(() => {
    const storage = codingSessionSeenStorage();
    if (onScreen) {
      const next = codingSessionDiffOpenedMarker(newest, Date.now());
      if (codingSessionDiffMarkerChanges(marker, next)) {
        writeCodingSessionDiffSeenMarker(storage, key, next);
      }
      return;
    }
    // No marker, or one this build cannot read: what is there now is seen.
    if (marker === null) {
      writeCodingSessionDiffSeenMarker(
        storage,
        key,
        codingSessionDiffBaselineMarker(newest, Date.now()),
      );
    }
  }, [key, marker, newest, onScreen]);

  const badge = React.useMemo(
    () => codingSessionDiffBadgeFromCtx(ctx, marker, newest),
    [ctx, marker, newest],
  );
  return (
    <CodingSessionSurfaceBadgePill badge={badge} slot={slot} surfaceId="diff" />
  );
}
