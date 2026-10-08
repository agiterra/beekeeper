import type * as React from "react";

import { cn } from "@/shared/lib/cn";
import type { SessionPreviewState } from "@/shared/api/tauriSessionPreview";

import type { SessionPreviewSlotControl } from "../hooks/useSessionPreviewSlot";

/**
 * Where the native view is drawn. The element itself stays empty: Rust places
 * the WKWebView over its rect (inset 2 px, so the dividers around it stay
 * grabbable). While an overlay covers it, or Rust has hidden it, the slot
 * paints the freeze frame Rust took at that moment, so the pane never goes
 * blank and never pretends to be live: the frame is marked as a still.
 */
export function SessionPreviewSlot({
  className,
  control,
  state,
}: {
  className?: string;
  control: SessionPreviewSlotControl;
  state: SessionPreviewState;
}) {
  const frozen = control.occluded || state.hidden || state.occluded;
  return (
    <div
      className={cn(
        "relative min-h-0 flex-1 overflow-hidden bg-background",
        className,
      )}
      data-frozen={frozen ? "true" : "false"}
      data-testid="session-preview-slot"
      ref={control.ref as React.Ref<HTMLDivElement>}
    >
      {frozen && state.freezeFrame ? (
        <img
          alt="Still of the page while a menu or dialog is open"
          className="pointer-events-none absolute inset-0 size-full select-none object-cover object-left-top"
          data-testid="session-preview-freeze-frame"
          draggable={false}
          src={state.freezeFrame}
        />
      ) : null}
      {state.status === "loading" && !frozen ? (
        <p className="absolute inset-x-0 top-1/2 text-center text-xs text-muted-foreground">
          Loading {state.url ?? "the page"}…
        </p>
      ) : null}
    </div>
  );
}
