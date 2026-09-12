import * as React from "react";

/**
 * How much room the composer dock actually needs at the bottom of the
 * conversation.
 *
 * The dock is absolutely positioned over the transcript, so the scroll column
 * has to reserve its height or the last thing in the conversation sits
 * underneath it. That reserve used to be the constant `pb-44`, and a constant
 * is wrong for an element whose height is a function of its contents: the
 * dock grows with a delivery-class hint, an unreachable notice, a wrapped
 * identity row, or simply larger text. When it outgrew the guess, the last
 * pending row's controls went under the dock and could not be clicked at all
 * — which is how a person loses the only exit from a message whose delivery
 * nobody can account for.
 *
 * Measured instead. Attach {@link CodingSessionDockReserve.ref} to the dock
 * and spread the returned `className`/`style` onto the scrolling column.
 * Before the first measurement — and anywhere `ResizeObserver` does not exist
 * — the old constant is the fallback, so nothing renders worse than it did.
 */
export const CODING_SESSION_DOCK_RESERVE_FALLBACK = "pb-44";

/** One line of breathing room above the dock's own top edge. */
const DOCK_RESERVE_GAP_PX = 16;

/** What the caller applies to the dock and to the column above it. */
export type CodingSessionDockReserve = {
  /** Attach to the dock element whose height is being reserved. */
  ref: React.RefObject<HTMLDivElement | null>;
  /** Tailwind fallback, or `undefined` once a measurement exists. */
  className: string | undefined;
  /** Measured reserve, or `undefined` before the first measurement. */
  style: { paddingBottom: string } | undefined;
};

/**
 * Reserve the dock's measured height at the foot of the scroll column.
 *
 * `railReserve` is the class to use instead while the task rail is open: the
 * rail is laid out above the dock and keeps its own, larger, reserve, so the
 * measurement is not what the column needs then.
 */
export function useCodingSessionDockReserve(
  railReserve?: string | false,
): CodingSessionDockReserve {
  const ref = React.useRef<HTMLDivElement>(null);
  const [height, setHeight] = React.useState<number | null>(null);
  React.useEffect(() => {
    const dock = ref.current;
    if (!dock || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(([entry]) => {
      const measured = entry?.borderBoxSize?.[0]?.blockSize;
      setHeight(
        typeof measured === "number"
          ? measured
          : dock.getBoundingClientRect().height,
      );
    });
    observer.observe(dock);
    return () => observer.disconnect();
  });
  if (railReserve) return { ref, className: railReserve, style: undefined };
  return {
    ref,
    className:
      height === null ? CODING_SESSION_DOCK_RESERVE_FALLBACK : undefined,
    style:
      height === null
        ? undefined
        : { paddingBottom: `${height + DOCK_RESERVE_GAP_PX}px` },
  };
}

const NARROW_CODING_SESSION_WORKSPACE_WIDTH = 960;

/**
 * `null` until the workspace width is first measured, so responsive chrome
 * (the surface host in particular) never mounts its desktop inline layout
 * only to be torn down and replaced by an animated sheet a frame later.
 */
export function useNarrowCodingSessionWorkspace(
  workspaceRef: React.RefObject<HTMLElement | null>,
): boolean | null {
  const [isNarrow, setIsNarrow] = React.useState<boolean | null>(null);

  React.useEffect(() => {
    const workspace = workspaceRef.current;
    if (!workspace) return;

    const update = (width: number) => {
      setIsNarrow(width < NARROW_CODING_SESSION_WORKSPACE_WIDTH);
    };
    update(workspace.getBoundingClientRect().width);

    if (typeof ResizeObserver === "undefined") {
      const media = window.matchMedia(
        `(max-width: ${NARROW_CODING_SESSION_WORKSPACE_WIDTH - 1}px)`,
      );
      const updateFromMedia = () => setIsNarrow(media.matches);
      media.addEventListener("change", updateFromMedia);
      return () => media.removeEventListener("change", updateFromMedia);
    }

    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) update(entry.contentRect.width);
    });
    observer.observe(workspace);
    return () => observer.disconnect();
  }, [workspaceRef]);

  return isNarrow;
}
