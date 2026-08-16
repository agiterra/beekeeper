import * as React from "react";

import { getStorageItem, setStorageItem } from "@/shared/lib/safeStorage";

const DEFAULT_WIDTH_PX = 360;
const MIN_WIDTH_PX = 288;
const MAX_WIDTH_PX = 720;
const MIN_NARRATIVE_WIDTH_PX = 420;
const STORAGE_KEY = "buzz.desktop.coding-session-rail-width";

export function clampCodingSessionRailWidth(
  width: number,
  containerWidth: number,
): number {
  const available = Math.max(
    MIN_WIDTH_PX,
    containerWidth - MIN_NARRATIVE_WIDTH_PX,
  );
  return Math.max(MIN_WIDTH_PX, Math.min(MAX_WIDTH_PX, available, width));
}

function initialWidth(): number {
  if (typeof window === "undefined") return DEFAULT_WIDTH_PX;
  const parsed = Number.parseInt(getStorageItem(STORAGE_KEY, "") ?? "", 10);
  return Number.isFinite(parsed) ? parsed : DEFAULT_WIDTH_PX;
}

export function useCodingSessionRailWidth(
  containerRef: React.RefObject<HTMLElement | null>,
) {
  const [width, setWidth] = React.useState(initialWidth);
  const widthRef = React.useRef(width);
  const activeDragCleanupRef = React.useRef<(() => void) | null>(null);
  widthRef.current = width;

  React.useEffect(
    () => () => {
      activeDragCleanupRef.current?.();
    },
    [],
  );

  const clampToContainer = React.useCallback(
    (candidate: number) =>
      clampCodingSessionRailWidth(
        candidate,
        containerRef.current?.clientWidth ?? window.innerWidth,
      ),
    [containerRef],
  );

  React.useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const resize = () => setWidth((current) => clampToContainer(current));
    const observer = new ResizeObserver(resize);
    observer.observe(container);
    window.addEventListener("resize", resize);
    resize();
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", resize);
    };
  }, [clampToContainer, containerRef]);

  const onResizeStart = React.useCallback(
    (event: React.PointerEvent<HTMLButtonElement>) => {
      if (event.button !== 0) return;
      event.preventDefault();
      event.currentTarget.setPointerCapture(event.pointerId);

      const handle = event.currentTarget;
      const pointerId = event.pointerId;
      const startX = event.clientX;
      const startWidth = widthRef.current;
      const previousCursor = document.body.style.cursor;
      const previousUserSelect = document.body.style.userSelect;
      let frame: number | null = null;
      let pendingWidth = startWidth;
      let settled = false;

      document.body.style.cursor = "col-resize";
      document.body.style.userSelect = "none";

      const commitFrame = () => {
        frame = null;
        setWidth(pendingWidth);
      };
      const move = (moveEvent: PointerEvent) => {
        pendingWidth = clampToContainer(
          startWidth - (moveEvent.clientX - startX),
        );
        if (frame === null) frame = requestAnimationFrame(commitFrame);
      };
      const finish = (persist: boolean, releaseCapture = true) => {
        if (settled) return;
        settled = true;
        if (frame !== null) cancelAnimationFrame(frame);
        const finalWidth = clampToContainer(
          persist ? pendingWidth : startWidth,
        );
        setWidth(finalWidth);
        if (persist) setStorageItem(STORAGE_KEY, String(finalWidth));
        document.body.style.cursor = previousCursor;
        document.body.style.userSelect = previousUserSelect;
        handle.removeEventListener("pointermove", move);
        handle.removeEventListener("pointerup", up);
        handle.removeEventListener("pointercancel", cancel);
        handle.removeEventListener("lostpointercapture", lostCapture);
        activeDragCleanupRef.current = null;
        if (releaseCapture && handle.hasPointerCapture(pointerId)) {
          handle.releasePointerCapture(pointerId);
        }
      };
      const up = () => finish(true);
      const cancel = () => finish(false);
      const lostCapture = () => finish(false, false);

      handle.addEventListener("pointermove", move);
      handle.addEventListener("pointerup", up, { once: true });
      handle.addEventListener("pointercancel", cancel, { once: true });
      handle.addEventListener("lostpointercapture", lostCapture, {
        once: true,
      });
      activeDragCleanupRef.current = cancel;
    },
    [clampToContainer],
  );

  const onResizeKeyDown = React.useCallback(
    (event: React.KeyboardEvent<HTMLButtonElement>) => {
      const delta =
        event.key === "ArrowLeft" ? 16 : event.key === "ArrowRight" ? -16 : 0;
      if (delta === 0) return;
      event.preventDefault();
      const next = clampToContainer(widthRef.current + delta);
      setWidth(next);
      setStorageItem(STORAGE_KEY, String(next));
    },
    [clampToContainer],
  );

  return { onResizeKeyDown, onResizeStart, width };
}
