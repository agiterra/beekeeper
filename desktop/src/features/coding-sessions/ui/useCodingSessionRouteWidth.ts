/**
 * The Mission Route rail's width, collapse, and the fold arithmetic that
 * governs both rails.
 *
 * Modelled line for line on `useCodingSessionRailWidth` — the Inspector's hook
 * — because L4.6's rule is that the route rail mirrors the Inspector control
 * for control. Two panels on one screen that resize by two different sets of
 * pointer, keyboard and persistence semantics is the kind of difference a
 * person feels and cannot name.
 *
 * Three things genuinely differ, and only these:
 *
 * 1. The bounds: 176–480, default 224, against the Inspector's 288–720/360.
 * 2. **The rail is on the LEFT.** Its handle is on its right edge, so a drag
 *    to the right *widens* it and the pointer delta is `clientX - startX`
 *    rather than the Inspector's `startX - clientX`. The arrow keys are
 *    mirrored to match: `ArrowRight` widens, `ArrowLeft` narrows. Getting this
 *    sign wrong produces a handle that runs away from the cursor, which reads
 *    as a broken control rather than an inverted one.
 * 3. A persisted collapsed flag, because collapsed is a state the Inspector
 *    expresses by closing and this rail expresses by becoming the 40 px
 *    scrubber.
 *
 * Persistence goes through `getStorageItem`/`setStorageItem`, which are the
 * throw-safe wrappers (`shared/lib/safeStorage`): WKWebView throws
 * `SecurityError` out of `localStorage.getItem` itself under a restricted
 * storage policy, and an unguarded read inside a `useState` initializer takes
 * the whole tree down with it.
 */
import * as React from "react";

import { getStorageItem, setStorageItem } from "@/shared/lib/safeStorage";

/** Exported so the rail's resizer can expose honest `aria-value*` semantics. */
export const CODING_SESSION_ROUTE_MIN_WIDTH_PX = 176;
export const CODING_SESSION_ROUTE_MAX_WIDTH_PX = 480;
export const CODING_SESSION_ROUTE_DEFAULT_WIDTH_PX = 224;
/**
 * Collapsed is the existing 40 px scrubber — it already carries the attention
 * signs, the band and Now. Nothing new is drawn for a folded rail.
 */
export const CODING_SESSION_ROUTE_COLLAPSED_WIDTH_PX = 40;
/**
 * The stream's floor, shared verbatim with the Inspector's hook. Below this a
 * reading column stops being a reading column.
 */
export const CODING_SESSION_STREAM_MIN_WIDTH_PX = 420;

const MIN_WIDTH_PX = CODING_SESSION_ROUTE_MIN_WIDTH_PX;
const MAX_WIDTH_PX = CODING_SESSION_ROUTE_MAX_WIDTH_PX;
const MIN_NARRATIVE_WIDTH_PX = CODING_SESSION_STREAM_MIN_WIDTH_PX;
const WIDTH_STORAGE_KEY = "buzz.desktop.coding-session-route-width";
const COLLAPSED_STORAGE_KEY = "buzz.desktop.coding-session-route-collapsed";

/**
 * Hold a dragged width inside the rail's bounds, leaving the stream its floor.
 *
 * A coarse floor on purpose: `containerWidth` here is the whole workspace
 * body, which still contains the Inspector, so this cannot know exactly what
 * the stream will be left with. It stops a drag from eating the window; the
 * fine decision — does the rail fold at all — is `codingSessionRouteFits` in
 * `lib/codingSessionRouteTypes`, which works off the section's real measured
 * width.
 */
export function clampCodingSessionRouteWidth(
  width: number,
  containerWidth: number,
): number {
  const available = Math.max(
    MIN_WIDTH_PX,
    containerWidth - MIN_NARRATIVE_WIDTH_PX,
  );
  return Math.max(MIN_WIDTH_PX, Math.min(MAX_WIDTH_PX, available, width));
}

/**
 * Strictly parse a persisted route width. Anything that is not a plain
 * base-10 integer inside the legal width range — trailing garbage, exponents,
 * negatives, absurd magnitudes — is rejected as corrupt rather than
 * "best-effort" coerced into layout, exactly as its Inspector twin does.
 */
export function parsePersistedCodingSessionRouteWidth(
  raw: string | null,
): number | null {
  if (raw === null || !/^\d{1,4}$/.test(raw)) return null;
  const parsed = Number(raw);
  if (!Number.isInteger(parsed)) return null;
  if (parsed < MIN_WIDTH_PX || parsed > MAX_WIDTH_PX) return null;
  return parsed;
}

/**
 * Strictly parse the persisted collapsed flag: only the literal `"1"` is
 * collapsed.
 *
 * An absent or unrecognised value means **not** collapsed. A rail that hides
 * itself because a stored byte was unreadable would look like the fold gate
 * misfiring, and the viewer would have no way to tell the two apart.
 */
export function parsePersistedCodingSessionRouteCollapsed(
  raw: string | null,
): boolean {
  return raw === "1";
}

function initialWidth(): number {
  if (typeof window === "undefined")
    return CODING_SESSION_ROUTE_DEFAULT_WIDTH_PX;
  return (
    parsePersistedCodingSessionRouteWidth(
      getStorageItem(WIDTH_STORAGE_KEY, null),
    ) ?? CODING_SESSION_ROUTE_DEFAULT_WIDTH_PX
  );
}

function initialCollapsed(): boolean {
  if (typeof window === "undefined") return false;
  return parsePersistedCodingSessionRouteCollapsed(
    getStorageItem(COLLAPSED_STORAGE_KEY, null),
  );
}

/**
 * Drive the Route rail's width and collapse for one Mission workspace.
 *
 * @param containerRef The workspace body, whose width bounds the drag.
 */
export function useCodingSessionRouteWidth(
  containerRef: React.RefObject<HTMLElement | null>,
) {
  const [width, setWidth] = React.useState(initialWidth);
  const [collapsed, setCollapsedState] = React.useState(initialCollapsed);
  const widthRef = React.useRef(width);
  const activeDragCleanupRef = React.useRef<(() => void) | null>(null);
  widthRef.current = width;

  React.useEffect(
    () => () => {
      activeDragCleanupRef.current?.();
    },
    [],
  );

  /**
   * Abort any in-flight drag and restore `document.body` cursor/user-select.
   * The host calls this on breakpoint transitions (inline rail → scrubber) so
   * a drag interrupted by a layout change never leaks global styles.
   */
  const cancelDrag = React.useCallback(() => {
    activeDragCleanupRef.current?.();
  }, []);

  const setCollapsed = React.useCallback((next: boolean) => {
    setCollapsedState(next);
    setStorageItem(COLLAPSED_STORAGE_KEY, next ? "1" : "0");
  }, []);

  const toggleCollapsed = React.useCallback(() => {
    setCollapsedState((current) => {
      const next = !current;
      setStorageItem(COLLAPSED_STORAGE_KEY, next ? "1" : "0");
      return next;
    });
  }, []);

  const clampToContainer = React.useCallback(
    (candidate: number) =>
      clampCodingSessionRouteWidth(
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
        // Mirrored from the Inspector: this rail's handle is on its RIGHT
        // edge, so rightward pointer travel adds width instead of removing it.
        pendingWidth = clampToContainer(
          startWidth + (moveEvent.clientX - startX),
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
        if (persist) setStorageItem(WIDTH_STORAGE_KEY, String(finalWidth));
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
      // Mirrored from the Inspector for the same reason the drag is: the rail
      // grows to the right, so ArrowRight widens and ArrowLeft narrows.
      const delta =
        event.key === "ArrowRight" ? 16 : event.key === "ArrowLeft" ? -16 : 0;
      if (delta === 0) return;
      event.preventDefault();
      const next = clampToContainer(widthRef.current + delta);
      setWidth(next);
      setStorageItem(WIDTH_STORAGE_KEY, String(next));
    },
    [clampToContainer],
  );

  return {
    cancelDrag,
    collapsed,
    onResizeKeyDown,
    onResizeStart,
    setCollapsed,
    toggleCollapsed,
    width,
  };
}
