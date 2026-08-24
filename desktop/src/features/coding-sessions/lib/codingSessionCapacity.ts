/**
 * The session-capacity setting, as a model rather than a form.
 *
 * Bee Keeper's provider holds at most a few live agent processes at once, and
 * hitting that ceiling reads as somebody else's limit: "there can only be 4
 * concurrent Claude sessions?" (reported 2026-08-24). It is this computer's
 * cap, it is now the person's to set, and this module owns the rules the panel
 * and the tests share.
 */

/** The stored value that means "no ceiling". */
export const CODING_SESSION_CAPACITY_UNLIMITED = 0;
/** Above this, a number is almost certainly a typo rather than an intention. */
export const CODING_SESSION_CAPACITY_MAX = 64;

export type CodingSessionCapacitySettings = {
  /** Stored ceiling: null for the provider default, 0 for unlimited. */
  maxSessions: number | null;
  /** The provider's own default, supplied by the host rather than guessed. */
  defaultMaxSessions: number;
  /** The ceiling the running provider started with, when one is running. */
  runningMaxSessions: number | null;
};

export type CodingSessionCapacityChoice =
  | { kind: "default" }
  | { kind: "unlimited" }
  | { kind: "limit"; value: number };

/** Which of the three states a stored value represents. */
export function codingSessionCapacityChoice(
  maxSessions: number | null,
): CodingSessionCapacityChoice {
  if (maxSessions === null) return { kind: "default" };
  if (maxSessions === CODING_SESSION_CAPACITY_UNLIMITED) {
    return { kind: "unlimited" };
  }
  return { kind: "limit", value: maxSessions };
}

/** The value to store for a choice. */
export function codingSessionCapacityValue(
  choice: CodingSessionCapacityChoice,
): number | null {
  if (choice.kind === "default") return null;
  if (choice.kind === "unlimited") return CODING_SESSION_CAPACITY_UNLIMITED;
  return choice.value;
}

/**
 * Coerce typed input into a storable limit.
 *
 * Empty, non-numeric and below-one input keeps the previous value rather than
 * silently becoming unlimited — 0 means unlimited *only* when chosen
 * deliberately, never as a side effect of clearing the field.
 */
export function parseCodingSessionCapacityInput(
  raw: string,
  previous: number,
): number {
  const parsed = Number.parseInt(raw.trim(), 10);
  if (!Number.isFinite(parsed) || parsed < 1) return previous;
  return Math.min(parsed, CODING_SESSION_CAPACITY_MAX);
}

/** How the current ceiling reads in a sentence. */
export function codingSessionCapacityLabel(
  maxSessions: number | null,
  defaultMaxSessions: number,
): string {
  const choice = codingSessionCapacityChoice(maxSessions);
  if (choice.kind === "unlimited") return "Unlimited";
  const value = choice.kind === "default" ? defaultMaxSessions : choice.value;
  return `${value} session${value === 1 ? "" : "s"}`;
}

/**
 * What the panel must disclose about a change not yet in force.
 *
 * The provider reads its ceiling from the environment once, at startup, so a
 * saved change is a promise about the *next* start. Saying nothing would make
 * the panel claim a ceiling that is not being enforced — the same class of
 * lie as a header reading Idle over a dead provider (§2 item 41).
 */
export function codingSessionCapacityPending(settings: {
  maxSessions: number | null;
  defaultMaxSessions: number;
  runningMaxSessions: number | null;
  providerRunning: boolean;
}): string | null {
  if (!settings.providerRunning) return null;
  const stored = settings.maxSessions ?? settings.defaultMaxSessions;
  const running = settings.runningMaxSessions ?? settings.defaultMaxSessions;
  if (stored === running) return null;
  return `The provider running now started with ${codingSessionCapacityLabel(
    running,
    settings.defaultMaxSessions,
  ).toLowerCase()}. Your change applies the next time it starts.`;
}
