import * as React from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import {
  type SessionPreviewRect,
  sessionPreviewSetOccluded,
  sessionPreviewSetRect,
} from "@/shared/api/tauriSessionPreview";

import {
  previewSlotOccluded,
  readPreviewOccluders,
} from "../lib/previewOcclusion";
import {
  createPreviewRectSync,
  nextPreviewRectSeq,
} from "../lib/previewRectSync";

/** Frames to keep re-measuring after a trigger, to follow CSS transitions. */
const SETTLE_FRAMES = 12;

/** This webview's window label (`main`, or a `coding-session-*` pop-out). */
export function sessionPreviewWindowLabel(): string {
  try {
    return getCurrentWindow().label;
  } catch {
    return "main";
  }
}

function slotRect(element: HTMLElement): SessionPreviewRect | null {
  const box = element.getBoundingClientRect();
  if (box.width <= 0 || box.height <= 0) return null;
  return { x: box.left, y: box.top, width: box.width, height: box.height };
}

export type SessionPreviewSlotControl = {
  /** Attach to the slot element. */
  ref: React.RefCallback<HTMLElement>;
  /** Whether an overlay covers the slot right now (as last sent to Rust). */
  occluded: boolean;
  /** Re-measure now: a drag moved the slot without resizing it. */
  remeasure: () => void;
};

/**
 * The native view's slot (WIRE-C4 § 3): follows the slot's DOM rect with a
 * ResizeObserver, window resize and scroll, plus a short frame burst after
 * each trigger, and sends it last-write-wins through `set_rect`. While the
 * slot is mounted it also watches for occlusion leases (`previewOcclusion`)
 * and sends `set_occluded` on every change. Unmount sends `rect: null` and
 * releases the occlusion, so a tab switch never leaves the view hidden or
 * drawn over another surface.
 */
export function useSessionPreviewSlot(input: {
  channelId: string;
  /** False while there is nothing to place (no state yet, unavailable). */
  enabled: boolean;
}): SessionPreviewSlotControl {
  const { channelId, enabled } = input;
  const [element, setElement] = React.useState<HTMLElement | null>(null);
  const [occluded, setOccluded] = React.useState(false);
  const remeasureRef = React.useRef<() => void>(() => {});

  React.useEffect(() => {
    if (!element || !enabled) return;
    const windowLabel = sessionPreviewWindowLabel();
    const sync = createPreviewRectSync({
      send: (rect, seq) =>
        sessionPreviewSetRect({ channelId, windowLabel, rect, seq }),
      requestFrame: (callback) => window.requestAnimationFrame(callback),
      cancelFrame: (handle) => window.cancelAnimationFrame(handle),
      nextSeq: () => nextPreviewRectSeq(),
    });
    let sentOccluded = false;
    let framesLeft = 0;
    let frame: number | null = null;
    let disposed = false;

    const sendOccluded = (next: boolean) => {
      if (next === sentOccluded) return;
      sentOccluded = next;
      setOccluded(next);
      void sessionPreviewSetOccluded(channelId, next).catch(() => {
        // Unknown is not "shown": ask again on the next check.
        sentOccluded = !next;
      });
    };

    const measure = (): boolean => {
      const rect = slotRect(element);
      sync.schedule(rect);
      const candidates = readPreviewOccluders(document, element);
      sendOccluded(previewSlotOccluded(rect, candidates));
      return candidates.length > 0;
    };

    const tick = () => {
      frame = null;
      if (disposed) return;
      const overlaysMounted = measure();
      framesLeft = Math.max(0, framesLeft - 1);
      // While any overlay is mounted it may still be moving into place
      // (popper positioning, open animations): keep checking every frame.
      if (framesLeft > 0 || overlaysMounted) {
        frame = window.requestAnimationFrame(tick);
      }
    };

    const trigger = () => {
      framesLeft = SETTLE_FRAMES;
      if (frame === null && !disposed) {
        frame = window.requestAnimationFrame(tick);
      }
    };
    remeasureRef.current = trigger;

    const resize = new ResizeObserver(trigger);
    resize.observe(element);
    resize.observe(document.documentElement);
    const mutations = new MutationObserver(trigger);
    mutations.observe(document.body, { childList: true, subtree: true });
    window.addEventListener("resize", trigger);
    window.addEventListener("scroll", trigger, true);
    trigger();

    return () => {
      disposed = true;
      remeasureRef.current = () => {};
      if (frame !== null) window.cancelAnimationFrame(frame);
      resize.disconnect();
      mutations.disconnect();
      window.removeEventListener("resize", trigger);
      window.removeEventListener("scroll", trigger, true);
      sync.dispose();
      void sessionPreviewSetRect({
        channelId,
        windowLabel,
        rect: null,
        seq: nextPreviewRectSeq(),
      }).catch(() => {});
      if (sentOccluded) {
        void sessionPreviewSetOccluded(channelId, false).catch(() => {});
      }
      setOccluded(false);
    };
  }, [channelId, element, enabled]);

  const ref = React.useCallback((node: HTMLElement | null) => {
    setElement(node);
  }, []);
  const remeasure = React.useCallback(() => remeasureRef.current(), []);
  return { ref, occluded, remeasure };
}
