import * as React from "react";

/**
 * How much empty space a coding session keeps at its left and right edges.
 *
 * - `full` — the gutter this feature shipped with: 20px, stepping to 32px once
 *   the window is wide enough for the `sm` breakpoint.
 * - `light` — half of `full` at each breakpoint, for people who would rather
 *   spend the window on the transcript than on margin.
 * - `none` — a flat 10px at every window width. Not literally zero: a
 *   transcript flush against the window edge is unreadable, and code blocks
 *   scroll horizontally inside it, so the sliver stays as a landing strip.
 *
 * Persisted in localStorage. This is a device-level UI preference, not
 * community-scoped data, so it is intentionally not reset on community switch.
 */
export type CodingSessionGutter = "full" | "light" | "none";

const STORAGE_KEY = "buzz.codingSessions.gutter";

/** Gutter used when nothing is stored, or the stored value is unrecognized. */
export const DEFAULT_CODING_SESSION_GUTTER: CodingSessionGutter = "full";

/**
 * The padding classes each choice resolves to.
 *
 * Every value is a rem-based Tailwind spacing token, never a px literal, for
 * the same reason the measure is `max-w-3xl`: Cmd +/- scales the root font
 * size, so a rem gutter grows with the glyphs it separates and a px one would
 * freeze against the zoom.
 */
export const CODING_SESSION_GUTTER_CLASSES: Record<
  CodingSessionGutter,
  string
> = {
  full: "px-5 sm:px-8",
  light: "px-2.5 sm:px-4",
  none: "px-2.5",
};

/** The choices offered in settings, in the order they are shown. */
export const CODING_SESSION_GUTTER_OPTIONS: {
  value: CodingSessionGutter;
  label: string;
  description: string;
}[] = [
  {
    value: "full",
    label: "Full",
    description:
      "The standard gutter — 32px on either side, narrowing to 20px in a small window.",
  },
  {
    value: "light",
    label: "Light",
    description:
      "Half of Full — 16px on either side, narrowing to 10px in a small window.",
  },
  {
    value: "none",
    label: "None",
    description:
      "A flat 10px on either side at every window width, so the transcript runs nearly edge to edge.",
  },
];

/** The stored string, narrowed to a choice this build still offers. */
export function parseCodingSessionGutter(
  value: string | null | undefined,
): CodingSessionGutter {
  return value === "full" || value === "light" || value === "none"
    ? value
    : DEFAULT_CODING_SESSION_GUTTER;
}

const listeners = new Set<() => void>();

let codingSessionGutter = readStoredCodingSessionGutter();

function readStoredCodingSessionGutter(): CodingSessionGutter {
  try {
    return parseCodingSessionGutter(
      globalThis.localStorage?.getItem(STORAGE_KEY),
    );
  } catch {
    return DEFAULT_CODING_SESSION_GUTTER;
  }
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getSnapshot(): CodingSessionGutter {
  return codingSessionGutter;
}

function getServerSnapshot(): CodingSessionGutter {
  return DEFAULT_CODING_SESSION_GUTTER;
}

/** Read the persisted coding-session gutter outside of React. */
export function getCodingSessionGutter(): CodingSessionGutter {
  return codingSessionGutter;
}

/** Update the coding-session gutter and notify every subscribed surface. */
export function setCodingSessionGutter(gutter: CodingSessionGutter): void {
  codingSessionGutter = gutter;

  try {
    globalThis.localStorage?.setItem(STORAGE_KEY, gutter);
  } catch {
    // Persistence is best-effort; the in-memory value still applies.
  }

  for (const listener of listeners) {
    listener();
  }
}

/** Which gutter the person chose for coding sessions. */
export function useCodingSessionGutter(): CodingSessionGutter {
  return React.useSyncExternalStore(subscribe, getSnapshot, getServerSnapshot);
}

/**
 * The gutter padding classes for the surface that owns the viewport edge.
 *
 * Every coding-session surface calls this and spreads the result onto the
 * ancestor of its `CodingSessionColumn` — transcript scroller, composer
 * overlay, goal row — so all of them move together when the choice changes.
 * Applying it to the measure box instead silently narrows the measure by the
 * padding, which is how the transcript text and the composer edge drifted out
 * of register.
 */
export function useCodingSessionColumnGutter(): string {
  return CODING_SESSION_GUTTER_CLASSES[useCodingSessionGutter()];
}
