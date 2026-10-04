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
  /**
   * The dock's measured border-box height in px, or `null` before the first
   * measurement and whenever no dock is mounted. Read by the jump-to-latest
   * pill, which must sit above the composer at every dock height (SV-14).
   */
  dockHeight: number | null;
};

/** Space between the top of the composer dock and the jump-to-latest pill. */
export const CODING_SESSION_JUMP_PILL_GAP_PX = 12;

/**
 * Where the jump-to-latest pill sits, measured up from the transcript pane's
 * bottom edge (SV-14).
 *
 * With a measured dock: its height plus a gap, so the pill clears the
 * composer however tall the composer has grown — a delivery hint, an
 * attachment strip, a wrapped deck, larger text. With a dock that has not
 * been measured yet (first frame, no `ResizeObserver`), the old fixed
 * `bottom-32`. With no dock at all (a closed or
 * untargeted session), just above the sandbox footer that takes its place.
 */
export function codingSessionJumpPillPosition(input: {
  dockHeight: number | null;
  hasDock: boolean;
}): { className: string; style: { bottom: string } | undefined } {
  // No dock means the sandbox footer (`CodingSessionWorkspaceSandboxFooter`,
  // one 2.5rem row) holds the foot of the pane instead: clear it.
  if (!input.hasDock) return { className: "bottom-16", style: undefined };
  if (input.dockHeight === null) {
    return { className: "bottom-32", style: undefined };
  }
  return {
    className: "",
    style: {
      bottom: `${Math.round(input.dockHeight + CODING_SESSION_JUMP_PILL_GAP_PX)}px`,
    },
  };
}

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
    if (!dock) {
      // The dock unmounted (the session closed): a stale height would hold
      // the pill above a composer that is no longer there.
      setHeight(null);
      return;
    }
    if (typeof ResizeObserver === "undefined") return;
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
  if (railReserve) {
    return {
      ref,
      className: railReserve,
      style: undefined,
      dockHeight: height,
    };
  }
  return {
    ref,
    dockHeight: height,
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

/**
 * Keep the transcript viewport bounded when fixed chrome no longer fits.
 * Only the stable shell scrolls additionally; the transcript keeps its
 * element, virtualizer and anchoring. Observe the fixed shell border box,
 * not its scrollHeight, so entering normal flow cannot grow the fit budget.
 */
export function useCodingSessionReflow(
  workspaceRef: React.RefObject<HTMLElement | null>,
  dockRef: React.RefObject<HTMLDivElement | null>,
): { active: boolean; height: number } {
  const [fit, setFit] = React.useState({ active: false, height: 0 });
  React.useLayoutEffect(() => {
    const workspace = workspaceRef.current;
    if (!workspace) return;
    const shell =
      workspace.closest<HTMLElement>('[data-testid="coding-session-shell"]') ??
      workspace;
    const handover = shell.querySelector<HTMLElement>(
      '[data-testid="coding-session-handover-slot"]',
    );
    const header = workspace.querySelector<HTMLElement>(
      '[data-testid="coding-session-authority-summary"]',
    );
    const goal = workspace.querySelector<HTMLElement>(
      '[data-testid="coding-session-goal-slot"]',
    );
    const dock = dockRef.current;
    const update = () => {
      const height = shell.clientHeight;
      if (height <= 0) return;
      const rootSize = Number.parseFloat(
        window.getComputedStyle(document.documentElement).fontSize,
      );
      const readingRoom = (Number.isFinite(rootSize) ? rootSize : 16) * 6;
      const chrome = [handover, header, goal, dock].reduce(
        (total, element) =>
          total + (element?.getBoundingClientRect().height ?? 0),
        0,
      );
      const active = chrome + readingRoom > height;
      setFit((previous) =>
        previous.active === active && previous.height === height
          ? previous
          : { active, height },
      );
    };
    update();
    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", update);
      return () => window.removeEventListener("resize", update);
    }
    const observer = new ResizeObserver(update);
    for (const element of [shell, handover, header, goal, dock]) {
      if (element) observer.observe(element);
    }
    return () => observer.disconnect();
  });
  return fit;
}
