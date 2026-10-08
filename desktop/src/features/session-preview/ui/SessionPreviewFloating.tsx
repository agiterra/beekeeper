import type * as React from "react";
import { createPortal } from "react-dom";
import { GripHorizontal, PanelRightClose, X } from "lucide-react";

import { Button } from "@/shared/ui/button";

import {
  type PreviewFloatBox,
  previewFloatDrag,
} from "../lib/previewFloatGeometry";
import { SESSION_PREVIEW_CHROME_ATTR } from "../lib/previewOcclusion";

function viewport() {
  return { width: window.innerWidth, height: window.innerHeight };
}

/**
 * The floating mini-player over the transcript: a fixed box in this window's
 * DOM, dragged by its header and resized from its corner. The native view
 * follows the slot inside it (Rust sees only `set_rect`; "floating" is a UI
 * placement). `onMoved` re-measures the slot, because a move changes no size
 * a ResizeObserver could see.
 */
export function SessionPreviewFloating({
  box,
  children,
  onBox,
  onClose,
  onDock,
  onMoved,
  title,
}: {
  box: PreviewFloatBox;
  children: React.ReactNode;
  onBox: (box: PreviewFloatBox) => void;
  onClose: () => void;
  onDock: () => void;
  onMoved: () => void;
  title: string;
}) {
  const startDrag =
    (mode: "move" | "resize") => (event: React.PointerEvent<HTMLElement>) => {
      if (event.button !== 0) return;
      if (
        mode === "move" &&
        (event.target as HTMLElement).closest("button") !== null
      ) {
        return;
      }
      event.preventDefault();
      const origin = { x: event.clientX, y: event.clientY };
      const start = box;
      const target = event.currentTarget;
      target.setPointerCapture(event.pointerId);
      const move = (next: PointerEvent) => {
        onBox(
          previewFloatDrag(
            start,
            mode,
            next.clientX - origin.x,
            next.clientY - origin.y,
            viewport(),
          ),
        );
        onMoved();
      };
      const end = () => {
        target.removeEventListener("pointermove", move);
        target.removeEventListener("pointerup", end);
        target.removeEventListener("pointercancel", end);
        onMoved();
      };
      target.addEventListener("pointermove", move);
      target.addEventListener("pointerup", end);
      target.addEventListener("pointercancel", end);
    };

  return createPortal(
    <section
      {...{ [SESSION_PREVIEW_CHROME_ATTR]: "" }}
      aria-label="Browser preview, floating"
      className="fixed z-40 flex flex-col overflow-hidden rounded-xl border border-border bg-background shadow-2xl"
      data-testid="session-preview-floating"
      style={{
        left: box.x,
        top: box.y,
        width: box.width,
        height: box.height,
      }}
    >
      <div
        className="flex h-7 shrink-0 cursor-grab touch-none select-none items-center gap-1 border-b border-border/60 bg-muted/40 px-2 active:cursor-grabbing"
        data-testid="session-preview-floating-handle"
        onPointerDown={startDrag("move")}
      >
        <GripHorizontal
          aria-hidden
          className="size-3.5 text-muted-foreground"
        />
        <span className="min-w-0 flex-1 truncate text-2xs text-muted-foreground">
          {title}
        </span>
        <Button
          aria-label="Dock in the panel"
          data-testid="session-preview-dock"
          onClick={onDock}
          size="icon-xs"
          variant="ghost"
        >
          <PanelRightClose />
        </Button>
        <Button
          aria-label="Close preview"
          data-testid="session-preview-floating-close"
          onClick={onClose}
          size="icon-xs"
          variant="ghost"
        >
          <X />
        </Button>
      </div>
      {children}
      <div
        aria-hidden
        className="absolute right-0 bottom-0 size-3 cursor-nwse-resize touch-none"
        data-testid="session-preview-floating-resize"
        onPointerDown={startDrag("resize")}
      />
    </section>,
    document.body,
  );
}
