import * as React from "react";

import { formatCodingSessionDuration } from "@/features/coding-sessions/lib/codingSessionTranscriptModel";

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
  return (
    <div
      className="px-0.5 pt-1 text-sm text-muted-foreground tabular-nums"
      data-testid="coding-session-working"
      ref={ref}
    >
      <CodingSessionWorkingTimer startedAt={startedAt} />
      {stepLabel ? (
        <span className="ml-2 text-muted-foreground/60">· {stepLabel}</span>
      ) : null}
      {showThinking ? <CodingSessionThinking /> : null}
    </div>
  );
}

function CodingSessionThinking() {
  return (
    <div
      className="relative mt-1 min-h-6 w-fit max-w-full overflow-hidden rounded-md text-sm leading-relaxed"
      data-testid="coding-session-thinking"
    >
      <span className="block py-0.5 text-muted-foreground/65">Thinking</span>
      <span
        aria-hidden
        className="coding-session-live-activity-focus pointer-events-none absolute inset-y-0 select-none"
      >
        <span className="coding-session-live-activity-counter block">
          <span className="coding-session-live-activity-aligned block py-0.5 text-foreground">
            Thinking
          </span>
        </span>
      </span>
    </div>
  );
}

/** Text of the working line at `now`; "Working…" until a start is known. */
export function formatCodingSessionWorkingLabel(
  startedAt: string | null,
  now: number,
): string {
  if (!startedAt) return "Working…";
  const start = Date.parse(startedAt);
  if (!Number.isFinite(start) || now <= start) return "Working…";
  return `Working for ${formatCodingSessionDuration(now - start)}`;
}

/** `Node.TEXT_NODE`, without reaching for a DOM global during render. */
const TEXT_NODE = 3;

function CodingSessionWorkingTimer({
  startedAt,
}: {
  startedAt: string | null;
}) {
  const ref = React.useRef<HTMLSpanElement>(null);
  const initial = formatCodingSessionWorkingLabel(startedAt, Date.now());

  React.useEffect(() => {
    if (!startedAt) return;
    const update = () => {
      const element = ref.current;
      if (!element) return;
      const text = formatCodingSessionWorkingLabel(startedAt, Date.now());
      // Write React's own text node rather than replacing it, so a later
      // render (a new step label, say) still updates the node on screen.
      const node = element.firstChild;
      if (node && node.nodeType === TEXT_NODE) {
        if (node.nodeValue !== text) node.nodeValue = text;
      } else {
        element.textContent = text;
      }
    };
    update();
    const interval = window.setInterval(update, 1_000);
    return () => window.clearInterval(interval);
  }, [startedAt]);

  return <span ref={ref}>{initial}</span>;
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
