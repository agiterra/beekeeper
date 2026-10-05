import { useCodingSessionRunningGates } from "@/features/coding-sessions/hooks/useCodingSessionGateStartClock";
import { codingSessionObservationNotLiveOf } from "@/features/coding-sessions/lib/codingSessionObservationLiveness";
import {
  codingSessionLandingBadgeFromCtx,
  formatCodingSessionSurfaceClock,
} from "@/features/coding-sessions/lib/codingSessionSurfaceBadgeModelCtx";
import { CodingSessionSurfaceBadgePill } from "../CodingSessionSurfaceBadgePill";
import type { CodingSessionSurfaceCtx } from "./codingSessionSurfaceContext";
import type { CodingSessionSurfaceBadgeSlot } from "./codingSessionSurfaceRegistry";

/**
 * The Landing surface's badge (SV-22, SV-41): a destructive dot when the
 * head's newest gate failed or the newest verdict refuses, amber while a
 * `decision.request` waits on a person, and "gate running, watched by
 * {provider}" while the provider has signed a gate start it has not closed —
 * its hover says when it began by the provider's clock. A start gone stale
 * is "no result observed", never running, so it draws nothing.
 */
export function CodingSessionSurfaceLandingBadge({
  ctx,
  slot,
}: {
  ctx: CodingSessionSurfaceCtx;
  slot: CodingSessionSurfaceBadgeSlot;
}) {
  const fold =
    ctx.observations.state === "read"
      ? (ctx.observations.result?.fold ?? null)
      : null;
  // A snapshot says so: each running gate carries "not live — read at HH:MM"
  // whenever the live subscription is not up.
  const runningGates = useCodingSessionRunningGates(
    fold,
    codingSessionObservationNotLiveOf(
      ctx.observations,
      formatCodingSessionSurfaceClock,
    ),
  );
  return (
    <CodingSessionSurfaceBadgePill
      badge={codingSessionLandingBadgeFromCtx(ctx, runningGates)}
      slot={slot}
      surfaceId="landing"
    />
  );
}
