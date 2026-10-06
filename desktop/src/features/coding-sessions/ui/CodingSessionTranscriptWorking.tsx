import type { CSSProperties } from "react";
import * as React from "react";

import { formatCodingSessionDuration } from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import {
  CODING_SESSION_QUIET_AFTER_MS,
  codingSessionQuietMs,
} from "@/features/coding-sessions/lib/codingSessionWaitingLiveness";
import {
  CodingSessionLiveShimmerText,
  useCodingSessionLiveShimmer,
} from "./CodingSessionTranscriptWorkingShimmer";

export { CODING_SESSION_QUIET_AFTER_MS, codingSessionQuietMs };

/**
 * The live turn's "Working for …" line and its Thinking shimmer.
 *
 * Split out of `CodingSessionTranscriptParts.tsx`. Two costs are kept off
 * React here:
 *
 * - the clock writes its own text node once a second instead of setting
 *   state, so a long turn does not commit a render per second;
 * - the shimmer (and any spinner inside the line) pauses while the line is
 *   scrolled out of view, so a session left open on an old turn does not
 *   keep compositing an animation nobody can see.
 *
 * Not a live region: a timer announced every second is noise. The
 * transcript's own `coding-session-live-status` says working or idle.
 *
 * When the provider has published nothing for over a minute, the line says
 * "· no update for Nm" (`data-quiet="true"`), measured from the newest
 * transcript event's own time — never from when this view mounted. It
 * clears with the next event.
 *
 * SV-104: the live text shimmers (the shared `Shimmer`) only while the
 * provider is fresh — "Thinking" when it is shown, the working label when it
 * is not, never both — and stops at "no update for Nm", when the turn
 * settles (this line unmounts) and under reduced motion.
 */
export function CodingSessionWorking({
  showThinking = false,
  startedAt,
  stepLabel = null,
}: {
  showThinking?: boolean;
  startedAt: string | null;
  stepLabel?: string | null;
}) {
  const ref = React.useRef<HTMLDivElement>(null);
  usePauseAnimationsOffscreen(ref);
  const lastEventAt = React.useContext(CodingSessionLastTranscriptEventContext);
  // This line is mounted only while its turn is working: live by construction.
  const shimmer = useCodingSessionLiveShimmer(true, lastEventAt);
  return (
    <div
      className="px-0.5 pt-1 text-sm text-muted-foreground tabular-nums"
      data-testid="coding-session-working"
      ref={ref}
    >
      <CodingSessionWorkingTimer
        shimmer={shimmer && !showThinking}
        startedAt={startedAt}
      />
      {stepLabel ? (
        <span className="ml-2 text-muted-foreground/60">· {stepLabel}</span>
      ) : null}
      {showThinking ? <CodingSessionThinking shimmer={shimmer} /> : null}
    </div>
  );
}

function CodingSessionThinking({ shimmer }: { shimmer: boolean }) {
  return (
    <div
      className="mt-1 min-h-6 w-fit max-w-full text-sm leading-relaxed"
      data-testid="coding-session-thinking"
    >
      <CodingSessionLiveShimmerText
        active={shimmer}
        className="block py-0.5 text-muted-foreground/65"
        text="Thinking"
      />
    </div>
  );
}

/**
 * The newest transcript event this execution has published, in ms of the
 * event's own time, or `null` when the caller cannot say.
 *
 * A context rather than a prop through every turn: it moves with every
 * event, and only the working line reads it, so a turn's memo is not broken
 * by it. Nothing module-level: `resetCommunityState()` has nothing to reset.
 */
export const CodingSessionLastTranscriptEventContext = React.createContext<
  number | null
>(null);

/**
 * Text of the working line at `now`; "Working…" until a start is known.
 * When the provider has published nothing for over a minute the line says
 * so, measured from `lastEventAt` — "Working for 5m 22s · no update for 4m".
 */
export function formatCodingSessionWorkingLabel(
  startedAt: string | null,
  now: number,
  lastEventAt: number | null = null,
): string {
  const start = startedAt ? Date.parse(startedAt) : Number.NaN;
  const known = Number.isFinite(start) && now > start;
  const quiet = codingSessionQuietMs(lastEventAt, now);
  if (quiet !== null) {
    const head = known
      ? `Working for ${formatCodingSessionDuration(now - start)}`
      : "Working";
    return `${head} · no update for ${formatCodingSessionDuration(quiet)}`;
  }
  return known
    ? `Working for ${formatCodingSessionDuration(now - start)}`
    : "Working…";
}

/** `Node.TEXT_NODE`, without reaching for a DOM global during render. */
const TEXT_NODE = 3;

function CodingSessionWorkingTimer({
  shimmer = false,
  startedAt,
}: {
  /**
   * Draw the shared `Shimmer` (its `buzz-shimmer` classes, so its
   * reduced-motion guard applies) over the label. The label is rewritten
   * once a second outside React, so the overlay's copy is rewritten with it.
   */
  shimmer?: boolean;
  startedAt: string | null;
}) {
  const ref = React.useRef<HTMLSpanElement>(null);
  const lastEventAt = React.useContext(CodingSessionLastTranscriptEventContext);
  const initialNow = Date.now();
  const initial = formatCodingSessionWorkingLabel(
    startedAt,
    initialNow,
    lastEventAt,
  );
  const initialQuiet = codingSessionQuietMs(lastEventAt, initialNow) !== null;

  React.useEffect(() => {
    if (!startedAt && lastEventAt === null) return;
    const update = () => {
      const element = ref.current;
      if (!element) return;
      const now = Date.now();
      const text = formatCodingSessionWorkingLabel(startedAt, now, lastEventAt);
      const quiet = codingSessionQuietMs(lastEventAt, now) !== null;
      if (element.dataset.quiet !== String(quiet)) {
        element.dataset.quiet = String(quiet);
      }
      // Write React's own text node rather than replacing it, so a later
      // render (a new step label, say) still updates the node on screen.
      const node = element.firstChild;
      if (node && node.nodeType === TEXT_NODE) {
        if (node.nodeValue !== text) node.nodeValue = text;
      } else {
        element.textContent = text;
      }
      const overlay = element.querySelector<HTMLElement>(
        ".buzz-shimmer-overlay",
      );
      // Same for the overlay: keep React's text node, rewrite its value.
      const overlayNode = overlay?.firstChild;
      if (overlayNode && overlayNode.nodeType === TEXT_NODE) {
        if (overlayNode.nodeValue !== text) overlayNode.nodeValue = text;
      } else if (overlay && overlay.textContent !== text) {
        overlay.textContent = text;
      }
    };
    update();
    const interval = window.setInterval(update, 1_000);
    return () => window.clearInterval(interval);
  }, [lastEventAt, startedAt]);

  if (!shimmer) {
    return (
      <span data-live-shimmer="off" data-quiet={String(initialQuiet)} ref={ref}>
        {initial}
      </span>
    );
  }
  return (
    <span
      className="buzz-shimmer"
      data-live-shimmer="on"
      data-quiet={String(initialQuiet)}
      ref={ref}
      style={
        {
          "--buzz-shimmer-spread": `${initial.length * 2}px`,
        } as CSSProperties
      }
    >
      {initial}
      {/* Visual-only highlight copy, as `Shimmer` draws it; the text node
          above is the sole accessible content. */}
      <span aria-hidden="true" className="buzz-shimmer-overlay">
        {initial}
      </span>
    </span>
  );
}

/**
 * Mark an element `data-offscreen="true"` while it is out of view; the
 * stylesheet (`coding-session.css`) pauses every animation inside it.
 */
function usePauseAnimationsOffscreen(ref: React.RefObject<HTMLElement | null>) {
  React.useEffect(() => {
    const element = ref.current;
    if (!element || typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver((entries) => {
      const entry = entries.at(-1);
      if (!entry) return;
      element.dataset.offscreen = entry.isIntersecting ? "false" : "true";
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [ref]);
}
