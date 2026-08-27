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
  /** Stored per-turn silence budget in seconds, or null for the default. */
  turnIdleTimeoutSecs: number | null;
  /** The provider's own default silence budget, in seconds. */
  defaultTurnIdleTimeoutSecs: number;
  /** The budget the running provider started with, when one is running. */
  runningTurnIdleTimeoutSecs: number | null;
  /** Stored crew turn budget: null for the provider default, 0 for unlimited. */
  turnBudget: number | null;
  /** The provider's own default crew turn budget. */
  defaultTurnBudget: number;
  /** The crew budget the running provider started with, when one is running. */
  runningTurnBudget: number | null;
};

/** The stored value that means "no crew turn budget". */
export const CODING_SESSION_TURN_BUDGET_UNLIMITED = 0;
/** Above this, a crew budget is not a budget any more. */
export const CODING_SESSION_TURN_BUDGET_MAX = 10_000;

/** Shortest silence budget worth offering: below this, ordinary thinking trips it. */
export const CODING_SESSION_IDLE_TIMEOUT_MIN_MINUTES = 1;
/** Longest: past this, a wedged turn outlives the person's patience anyway. */
export const CODING_SESSION_IDLE_TIMEOUT_MAX_MINUTES = 240;

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

/**
 * Coerce typed minutes into a storable silence budget.
 *
 * Same rule as the capacity field: unusable input keeps the previous value
 * rather than silently becoming something the person did not choose.
 */
export function parseCodingSessionIdleTimeoutInput(
  raw: string,
  previousMinutes: number,
): number {
  const parsed = Number.parseInt(raw.trim(), 10);
  if (
    !Number.isFinite(parsed) ||
    parsed < CODING_SESSION_IDLE_TIMEOUT_MIN_MINUTES
  ) {
    return previousMinutes;
  }
  return Math.min(parsed, CODING_SESSION_IDLE_TIMEOUT_MAX_MINUTES);
}

/** Seconds as the minutes a person set, rounded up so nothing reads as zero. */
export function codingSessionIdleTimeoutMinutes(seconds: number): number {
  return Math.max(1, Math.round(seconds / 60));
}

/** How the current silence budget reads in a sentence. */
export function codingSessionIdleTimeoutLabel(
  turnIdleTimeoutSecs: number | null,
  defaultTurnIdleTimeoutSecs: number,
): string {
  const seconds = turnIdleTimeoutSecs ?? defaultTurnIdleTimeoutSecs;
  const minutes = codingSessionIdleTimeoutMinutes(seconds);
  return `${minutes} minute${minutes === 1 ? "" : "s"}`;
}

/**
 * What the panel must disclose about a silence budget not yet in force.
 *
 * Same reason as {@link codingSessionCapacityPending}: the child reads it from
 * the environment once, at startup.
 */
export function codingSessionIdleTimeoutPending(settings: {
  turnIdleTimeoutSecs: number | null;
  defaultTurnIdleTimeoutSecs: number;
  runningTurnIdleTimeoutSecs: number | null;
  providerRunning: boolean;
}): string | null {
  if (!settings.providerRunning) return null;
  const stored =
    settings.turnIdleTimeoutSecs ?? settings.defaultTurnIdleTimeoutSecs;
  const running =
    settings.runningTurnIdleTimeoutSecs ?? settings.defaultTurnIdleTimeoutSecs;
  if (stored === running) return null;
  return `The provider running now gives up after ${codingSessionIdleTimeoutLabel(
    running,
    settings.defaultTurnIdleTimeoutSecs,
  )} of silence. Your change applies the next time it starts.`;
}

/**
 * Coerce typed input into a storable crew turn budget.
 *
 * Same rule as the ceiling: unusable input keeps the previous value, so 0
 * ("no budget") is only ever reached by choosing it, never by clearing the
 * field.
 */
export function parseCodingSessionTurnBudgetInput(
  raw: string,
  previous: number,
): number {
  const parsed = Number.parseInt(raw.trim(), 10);
  if (!Number.isFinite(parsed) || parsed < 1) return previous;
  return Math.min(parsed, CODING_SESSION_TURN_BUDGET_MAX);
}

/** How the current crew turn budget reads in a sentence. */
export function codingSessionTurnBudgetLabel(
  turnBudget: number | null,
  defaultTurnBudget: number,
): string {
  const value = turnBudget ?? defaultTurnBudget;
  if (value === CODING_SESSION_TURN_BUDGET_UNLIMITED) return "No limit";
  return `${value} turn${value === 1 ? "" : "s"}`;
}

/**
 * What the panel must disclose about a crew budget not yet in force.
 *
 * Same reason as {@link codingSessionCapacityPending}: the child reads it from
 * the environment once, at startup, so a saved change is a promise about the
 * next start rather than a claim about the crew running now.
 */
export function codingSessionTurnBudgetPending(settings: {
  turnBudget: number | null;
  defaultTurnBudget: number;
  runningTurnBudget: number | null;
  providerRunning: boolean;
}): string | null {
  if (!settings.providerRunning) return null;
  const stored = settings.turnBudget ?? settings.defaultTurnBudget;
  const running = settings.runningTurnBudget ?? settings.defaultTurnBudget;
  if (stored === running) return null;
  return `The provider running now allows ${codingSessionTurnBudgetLabel(
    running,
    settings.defaultTurnBudget,
  ).toLowerCase()} per crew session. Your change applies the next time it starts.`;
}

/**
 * How a crew's spend reads beside its allowance, for the session Info popover.
 *
 * Says what was spent and what is left in the same breath, and names the
 * over-spent case rather than showing a negative remainder: the founder is
 * never refused, so a crew genuinely can end up past its allowance and a
 * surface that clamped it would be lying about who spent what.
 */
export function codingSessionTurnBudgetUsage(budget: {
  used: number;
  limit: number;
}): string {
  const remaining = budget.limit - budget.used;
  if (remaining > 0) {
    return `${budget.used} of ${budget.limit} turns used (${remaining} left)`;
  }
  if (remaining === 0) {
    return `${budget.used} of ${budget.limit} turns used (none left)`;
  }
  return `${budget.used} of ${budget.limit} turns used (${-remaining} over)`;
}
