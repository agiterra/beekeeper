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
 *
 * When the provider has published nothing for over a minute, the line says
 * "· no update for Nm" (`data-quiet="true"`), measured from the newest
 * transcript event's own time — never from when this view mounted. It
 * clears with the next event.
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
 * How long a working session may publish nothing before the working line says
 * so. Past this, "Working for 5m" over a provider that has been silent for
 * four of them is a comfortable guess, not a fact (2026-10-05).
 */
export const CODING_SESSION_QUIET_AFTER_MS = 60_000;

/**
 * How long the provider has been silent at `now`, floored to whole minutes,
 * or `null` while it is not past {@link CODING_SESSION_QUIET_AFTER_MS} — or
 * when no last event time is known, which is no reason to claim silence.
 */
export function codingSessionQuietMs(
  lastEventAt: number | null,
  now: number,
): number | null {
  if (lastEventAt === null || !Number.isFinite(lastEventAt)) return null;
  const quiet = now - lastEventAt;
  if (quiet <= CODING_SESSION_QUIET_AFTER_MS) return null;
  return Math.floor(quiet / 60_000) * 60_000;
}

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
  startedAt,
}: {
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
    };
    update();
    const interval = window.setInterval(update, 1_000);
    return () => window.clearInterval(interval);
  }, [lastEventAt, startedAt]);

  return (
    <span data-quiet={String(initialQuiet)} ref={ref}>
      {initial}
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
