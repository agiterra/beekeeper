import * as React from "react";

import {
  CODING_SESSION_TERMINAL_KEY_STEP,
  CODING_SESSION_TERMINAL_MIN_HEIGHT,
  clampCodingSessionTerminalHeight,
  codingSessionTerminalHeightStorageKey,
  codingSessionTerminalMaxHeight,
  parseStoredCodingSessionTerminalHeight,
} from "@/features/coding-sessions/lib/codingSessionTerminalModel";

function viewportHeight(): number {
  return typeof window === "undefined" ? 0 : window.innerHeight;
}

function readStoredHeight(key: string): number {
  try {
    return parseStoredCodingSessionTerminalHeight(
      window.localStorage.getItem(key),
    );
  } catch {
    return parseStoredCodingSessionTerminalHeight(null);
  }
}

function writeStoredHeight(key: string, height: number): void {
  try {
    window.localStorage.setItem(key, String(height));
  } catch {
    // Storage unavailable: the height still holds for this view.
  }
}

/**
 * The drawer's height (SV-25): T3's 280 px default, 180 px minimum and 75%
 * maximum (`ThreadTerminalDrawer.tsx:93-105`), changed by dragging the top
 * edge or with the arrow keys on it, and kept in `localStorage` under a key
 * that carries the community's relay URL so it survives a reload. A window
 * resize re-clamps it, as in T3.
 */
export function useCodingSessionTerminalHeight(relayUrl: string): {
  height: number;
  handleProps: React.HTMLAttributes<HTMLDivElement> & {
    role: "separator";
    tabIndex: number;
  };
} {
  const key = codingSessionTerminalHeightStorageKey(relayUrl);
  const [height, setHeight] = React.useState(() =>
    clampCodingSessionTerminalHeight(readStoredHeight(key), viewportHeight()),
  );
  const heightRef = React.useRef(height);
  heightRef.current = height;
  const dragRef = React.useRef<{
    pointerId: number;
    startY: number;
    startHeight: number;
  } | null>(null);

  React.useEffect(() => {
    setHeight(
      clampCodingSessionTerminalHeight(readStoredHeight(key), viewportHeight()),
    );
  }, [key]);

  React.useEffect(() => {
    const onResize = () =>
      setHeight((current) =>
        clampCodingSessionTerminalHeight(current, viewportHeight()),
      );
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  const commit = React.useCallback(
    (next: number) => {
      const clamped = clampCodingSessionTerminalHeight(next, viewportHeight());
      setHeight(clamped);
      writeStoredHeight(key, clamped);
    },
    [key],
  );

  const handleProps = {
    "aria-label": "Resize the terminal drawer",
    "aria-orientation": "horizontal" as const,
    "aria-valuemax": codingSessionTerminalMaxHeight(viewportHeight()),
    "aria-valuemin": CODING_SESSION_TERMINAL_MIN_HEIGHT,
    "aria-valuenow": height,
    role: "separator" as const,
    tabIndex: 0,
    onKeyDown: (event: React.KeyboardEvent<HTMLDivElement>) => {
      const current = heightRef.current;
      const step = event.shiftKey
        ? CODING_SESSION_TERMINAL_KEY_STEP * 4
        : CODING_SESSION_TERMINAL_KEY_STEP;
      let next: number | null = null;
      if (event.key === "ArrowUp") next = current + step;
      else if (event.key === "ArrowDown") next = current - step;
      else if (event.key === "Home") next = CODING_SESSION_TERMINAL_MIN_HEIGHT;
      else if (event.key === "End") {
        next = codingSessionTerminalMaxHeight(viewportHeight());
      }
      if (next === null) return;
      event.preventDefault();
      commit(next);
    },
    onPointerDown: (event: React.PointerEvent<HTMLDivElement>) => {
      if (event.button !== 0) return;
      event.preventDefault();
      event.currentTarget.setPointerCapture(event.pointerId);
      dragRef.current = {
        pointerId: event.pointerId,
        startY: event.clientY,
        startHeight: heightRef.current,
      };
    },
    onPointerMove: (event: React.PointerEvent<HTMLDivElement>) => {
      const drag = dragRef.current;
      if (!drag || drag.pointerId !== event.pointerId) return;
      event.preventDefault();
      setHeight(
        clampCodingSessionTerminalHeight(
          drag.startHeight + (drag.startY - event.clientY),
          viewportHeight(),
        ),
      );
    },
    onPointerUp: (event: React.PointerEvent<HTMLDivElement>) => {
      const drag = dragRef.current;
      if (!drag || drag.pointerId !== event.pointerId) return;
      dragRef.current = null;
      if (event.currentTarget.hasPointerCapture(event.pointerId)) {
        event.currentTarget.releasePointerCapture(event.pointerId);
      }
      commit(heightRef.current);
    },
    onPointerCancel: (event: React.PointerEvent<HTMLDivElement>) => {
      if (dragRef.current?.pointerId !== event.pointerId) return;
      dragRef.current = null;
      commit(heightRef.current);
    },
  };
  return { height, handleProps };
}
