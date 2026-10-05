import { ChevronDown } from "lucide-react";
import type * as React from "react";

import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";

import {
  codingSessionJumpPillPosition,
  useCodingSessionAwayFromEnd,
} from "../hooks/useCodingSessionWorkspaceLayout";

/**
 * The jump-to-latest pill both session layouts draw while the reader is away
 * from the end (SV-14; T3 `ChatView`'s "Scroll to end" pill).
 *
 * It floats over the transcript at `position` — the measured composer dock
 * plus a gap — so it sits above the composer at every dock height, never on
 * it. In a reflowed shell (fixed chrome taller than the window) nothing
 * floats: the pill takes its own row in the flow instead.
 *
 * `newCount` is the number of rows that arrived while the reader was away,
 * where the caller counts them; without a count it names what it does.
 */
export function CodingSessionWorkspaceJumpPill({
  inFlow = false,
  newCount = 0,
  onJump,
  position,
}: {
  inFlow?: boolean;
  newCount?: number;
  onJump: () => void;
  position: ReturnType<typeof codingSessionJumpPillPosition>;
}) {
  return (
    <div
      className={cn(
        inFlow
          ? "flex justify-center py-2"
          : "pointer-events-none absolute inset-x-0 z-30 flex justify-center",
        !inFlow && position.className,
      )}
      data-testid="coding-session-scroll-to-latest-slot"
      style={inFlow ? undefined : position.style}
    >
      <Button
        className="pointer-events-auto h-7 gap-1 rounded-full bg-background/90 px-2.5 text-xs shadow-md backdrop-blur-xl"
        data-testid="coding-session-scroll-to-latest"
        onClick={onJump}
        // Keep focus in the composer, as T3 does: the pill is a scroll, not a
        // place to type.
        onPointerDown={(event) => event.preventDefault()}
        size="sm"
        type="button"
        variant="outline"
      >
        <ChevronDown aria-hidden className="size-3.5" />
        {newCount > 0 ? `${newCount} new` : "Scroll to latest"}
      </Button>
    </div>
  );
}

/**
 * The pill for a narrative whose scroller the caller binds itself (the
 * umbrella workspace's bottom anchor): it watches `scrollRef` and shows while
 * the reader is away from the end, above a dock of `dockHeightPx` (0 before
 * the dock is measured), or above the closed footer when there is no dock.
 */
export function CodingSessionNarrativeJumpPill({
  dockHeightPx,
  hasDock,
  scrollRef,
}: {
  dockHeightPx: number;
  hasDock: boolean;
  scrollRef: React.RefObject<HTMLElement | null>;
}) {
  const away = useCodingSessionAwayFromEnd(scrollRef);
  if (!away) return null;
  return (
    <CodingSessionWorkspaceJumpPill
      onJump={() => {
        const scroller = scrollRef.current;
        scroller?.scrollTo({ behavior: "smooth", top: scroller.scrollHeight });
      }}
      position={codingSessionJumpPillPosition({
        dockHeight: dockHeightPx > 0 ? dockHeightPx : null,
        hasDock,
      })}
    />
  );
}
