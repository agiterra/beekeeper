import { codingSessionRunningCommandsLabel } from "@/features/coding-sessions/lib/codingSessionTerminalModel";
import { cn } from "@/shared/lib/cn";

import type { CodingSessionSurfaceCtx } from "./codingSessionSurfaceContext";
import type { CodingSessionSurfaceBadgeSlot } from "./codingSessionSurfaceRegistry";
import { codingSessionTerminalExtension } from "./CodingSessionSurfaceTerminalExtension";

/**
 * The Terminal surface's badge (SV-22, DB5, DB6): an activity count of this
 * session's shells **on this computer** that are running a command right
 * now — the terminal's foreground process group is not the login shell's,
 * read by Rust. An idle open shell is not work happening now and is not
 * counted; at the prompt the badge clears. Teammates' shared shells are
 * never counted, because NIP-ST carries no foreground fact; the label says
 * "on this computer" so the count claims nothing it did not read.
 *
 * Its tone is `data-tone="activity"`, the attribute the header's off-screen
 * probe reads, so a closed drawer's dot draws live work in the same tone the
 * badge does rather than as a neutral "something new".
 */
export function CodingSessionSurfaceTerminalBadge({
  ctx,
  slot,
}: {
  ctx: CodingSessionSurfaceCtx;
  slot: CodingSessionSurfaceBadgeSlot;
}) {
  const count = codingSessionTerminalExtension(ctx)?.runningCount ?? 0;
  if (count <= 0) return null;
  const label = codingSessionRunningCommandsLabel(count);
  return (
    <span
      aria-label={label}
      className={cn(
        "flex min-w-3.5 items-center justify-center rounded-full bg-primary px-1 font-semibold leading-none text-primary-foreground tabular-nums",
        slot === "header" ? "h-4 text-2xs" : "h-3.5 text-3xs",
      )}
      data-tone="activity"
      data-testid="coding-session-surface-badge-terminal"
      role="status"
      title={label}
    >
      {count}
    </span>
  );
}
