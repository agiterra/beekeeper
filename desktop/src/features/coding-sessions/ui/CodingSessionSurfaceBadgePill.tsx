import { cn } from "@/shared/lib/cn";
import {
  codingSessionSurfaceBadgeLabel,
  type CodingSessionSurfaceBadge,
  type CodingSessionSurfaceBadgeTone,
} from "@/features/coding-sessions/lib/codingSessionSurfaceBadgeModel";
import type { CodingSessionSurfaceBadgeSlot } from "./surfaces/codingSessionSurfaceRegistry";

/**
 * The pill on a surface icon's corner (SV-22, ref `sv22-t3-surface-badge`).
 *
 * The launcher row and the tab put it at the icon's top-right; this draws
 * only the pill. Its `aria-label` states the fact ("2 subagents running",
 * "Waiting on a ruling from you") and its hover title repeats it with any
 * detail one step away — the number alone is never the whole story.
 */
const TONE_CLASS: Readonly<Record<CodingSessionSurfaceBadgeTone, string>> = {
  activity: "bg-primary text-primary-foreground",
  waiting: "bg-amber-500 text-white",
  attention: "bg-destructive",
  neutral: "border border-border bg-muted text-muted-foreground",
};

export function CodingSessionSurfaceBadgePill({
  badge,
  slot,
  surfaceId,
}: {
  badge: CodingSessionSurfaceBadge | null;
  slot: CodingSessionSurfaceBadgeSlot;
  surfaceId: string;
}) {
  if (badge === null) return null;
  const label = codingSessionSurfaceBadgeLabel(badge);
  const title = badge.detail ? `${label}.\n${badge.detail}` : label;
  const dot = badge.count === null;
  return (
    <span
      aria-label={label}
      className={cn(
        "pointer-events-auto flex items-center justify-center rounded-full ring-2 ring-background",
        dot
          ? "size-2"
          : "h-3.5 min-w-3.5 px-1 text-3xs font-semibold leading-none tabular-nums",
        TONE_CLASS[badge.tone],
      )}
      data-slot={slot}
      data-testid={`coding-session-surface-badge-${surfaceId}`}
      data-tone={badge.tone}
      role="img"
      title={title}
    >
      {dot ? null : String(badge.count)}
    </span>
  );
}
