import { codingSessionDeviceOpenCount } from "@/features/coding-sessions/lib/codingSessionDevice";
import { cn } from "@/shared/lib/cn";

import type { CodingSessionSurfaceCtx } from "./codingSessionSurfaceContext";
import type { CodingSessionDeviceExtension } from "./CodingSessionSurfaceDevice";
import type { CodingSessionSurfaceBadgeSlot } from "./codingSessionSurfaceRegistry";

/**
 * The Device surface's badge (SV-22): how many of this session's device
 * slots are open now, by the providers' own newest `state` records. A
 * booting, closed or failed slot is not counted; with none open, no badge.
 */
export function CodingSessionSurfaceDeviceBadge({
  ctx,
  slot,
}: {
  ctx: CodingSessionSurfaceCtx;
  slot: CodingSessionSurfaceBadgeSlot;
}) {
  const value = ctx.extensions.device;
  const extension =
    value && typeof value === "object"
      ? (value as CodingSessionDeviceExtension)
      : null;
  const count = extension ? codingSessionDeviceOpenCount(extension.fold) : 0;
  if (count <= 0) return null;
  const label = count === 1 ? "1 device open" : `${count} devices open`;
  return (
    <span
      aria-label={label}
      className={cn(
        "flex min-w-3.5 items-center justify-center rounded-full bg-primary px-1 font-semibold leading-none text-primary-foreground tabular-nums",
        slot === "header" ? "h-4 text-2xs" : "h-3.5 text-3xs",
      )}
      data-testid="coding-session-surface-badge-device"
      data-tone="activity"
      role="status"
      title={label}
    >
      {count}
    </span>
  );
}
