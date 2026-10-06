import { parseRateLimitHint } from "@/shared/api/relayRateLimitGate";

/**
 * Retrying a `grant-operator` the relay asked this host to slow down on.
 *
 * A hire publishes six 44228 writes back to back — the launch's three and the
 * hire host's three — and the relay's per-pubkey admission budget is spent by
 * roughly that burst. On 2026-08-31 the fourth write came back `rate-limited:
 * quota exceeded; retry in 2s`, the host disclosed the failure and stopped,
 * and a seated agent was left mute over a two-second wait (item 103, finding
 * 3). Back-pressure is a "come back later", never a refusal; every other
 * failure is answered once, because retrying an answer that will not change
 * only delays telling the person.
 *
 * Two bounds keep the cure from being worse than the disease:
 *
 * 1. **Only a grant that failed to *publish* is retried.** A rate-limited
 *    confirmation read is waited out and re-read where it happens, inside
 *    `ensureCodingSessionOperatorGrant` — re-entering the whole grant to
 *    answer a refused read would sign a second 44228 for a write that already
 *    landed, multiplying the exact pressure this exists to survive.
 * 2. **A grant may not wait longer than
 *    {@link CODING_SESSION_GRANT_WAIT_BUDGET_MS}.** The relay clamps its own
 *    `retry in Ns` hint at 300 s, and four such waits would hold a hire's
 *    disclosure for twenty minutes while a lead waited on a seat's report.
 */

/** How many times one grant may be attempted before the host discloses. */
export const CODING_SESSION_GRANT_MAX_ATTEMPTS = 5;

/**
 * The wall-clock ceiling on one grant's retrying, measured from the first
 * attempt.
 *
 * Read from the clock rather than summed from the sleeps, so time spent
 * *inside* a slow attempt counts against it too. Only the final attempt can
 * run past it.
 */
export const CODING_SESSION_GRANT_WAIT_BUDGET_MS = 60_000;

/** The waits between attempts when the relay names no `retry in Ns` hint. */
const CODING_SESSION_GRANT_BACKOFF_MS = [2_000, 4_000, 8_000, 16_000] as const;

/** A grant that never landed: what it said, and how many tries it took. */
export type CodingSessionGrantFailure = {
  /** The failing attempt's own words, verbatim. */
  reason: string;
  /** Attempts actually made, 1-based. */
  attempts: number;
  /**
   * True when the retrying stopped because
   * {@link CODING_SESSION_GRANT_WAIT_BUDGET_MS} was spent rather than because
   * the attempt ceiling was reached. Disclosed, because "we stopped waiting"
   * and "the relay refused five times" are different facts.
   */
  waitBudgetSpent: boolean;
};

/**
 * The relay's own rate-limit answer, recognised by its shape.
 *
 * The relay emits exactly three of these — `rate-limited: quota exceeded;
 * retry in Ns`, `rate-limited: too many concurrent requests`, `rate-limited:
 * shared admission unavailable` (`crates/beekeeper-relay/src/connection.rs`) — and
 * the HTTP bridge's `relay rate-limited: retry in Ns`. All four carry the
 * `rate-limited:` token, so that token is what is matched.
 *
 * Deliberately **not** "any message containing a `retry in Ns` hint": a
 * refusal that happens to suggest trying later (`forbidden: not a founder;
 * retry in 5s`) is still a refusal, and retrying it five times would only
 * delay telling the person what the relay already decided.
 */
export function isCodingSessionGrantRateLimited(error: unknown): boolean {
  const message = (
    error instanceof Error ? error.message : String(error)
  ).toLowerCase();
  return message.includes("rate-limited:");
}

/**
 * How long to wait before the attempt after `attempt`.
 *
 * The relay's own hint wins when it published one — it knows when its window
 * reopens and this host does not. Anything that is not literally `retry in Ns`
 * falls back to the default 2 s → 16 s ladder: guessing a delay out of
 * arbitrary prose would be inventing a number and presenting it as the
 * relay's. Either way the answer is clamped to what is left of the grant's
 * wait budget, and 0 means "do not wait again".
 *
 * @param reason What the failed attempt said.
 * @param attempt Attempts made so far, 1-based.
 * @param remainingMs What is left of {@link CODING_SESSION_GRANT_WAIT_BUDGET_MS}.
 */
export function codingSessionGrantRetryDelayMs(
  reason: string,
  attempt: number,
  remainingMs: number = CODING_SESSION_GRANT_WAIT_BUDGET_MS,
): number {
  const hintSeconds = parseRateLimitHint(reason);
  const index = Math.min(
    Math.max(attempt, 1),
    CODING_SESSION_GRANT_BACKOFF_MS.length,
  );
  const wantedMs =
    hintSeconds !== null && hintSeconds > 0
      ? hintSeconds * 1_000
      : (CODING_SESSION_GRANT_BACKOFF_MS[index - 1] as number);
  return Math.min(wantedMs, Math.max(remainingMs, 0));
}

/**
 * The grant's own words, and — only when it was actually retried — what the
 * retrying spent.
 *
 * A single failed attempt keeps the exact sentence the relay said, so the
 * disclosure copy an operator already knows does not change shape for the
 * ordinary case. When the wait budget is what stopped it, the sentence says
 * so: a person reading "not granted" needs to tell a relay that refused five
 * times from a host that gave up waiting.
 */
export function codingSessionGrantFailureDetail(
  failure: CodingSessionGrantFailure,
): string {
  if (failure.attempts <= 1) return failure.reason;
  const seconds = CODING_SESSION_GRANT_WAIT_BUDGET_MS / 1_000;
  return failure.waitBudgetSpent
    ? `${failure.reason} (after ${failure.attempts} attempts and the ${seconds}s retry ceiling)`
    : `${failure.reason} (after ${failure.attempts} attempts)`;
}

/** What a failed grant said, never an empty string. */
export function codingSessionGrantFailureReason(error: unknown): string {
  const said = error instanceof Error ? error.message.trim() : String(error);
  return said.length > 0 ? said : "the grant did not go out";
}

/**
 * Run one grant, retrying while — and only while — the relay is applying
 * back-pressure to the write.
 *
 * Resolves null when the grant landed, or with the failing attempt's words,
 * the attempt count, and whether the wait budget is what stopped it. Never
 * throws.
 *
 * @param input.grant One grant attempt. Must be safe to re-run: a rate-limited
 *   *confirmation* is the callee's to wait out, so re-entering here means the
 *   write never went out.
 * @param input.sleep Injected in tests; real time by default.
 * @param input.now The clock the wait budget is measured on. `Date.now` by
 *   default; a test that injects `sleep` should inject this too, or the budget
 *   never binds.
 */
export async function ensureCodingSessionGrantWithBackoff(input: {
  grant: () => Promise<void>;
  sleep?: (milliseconds: number) => Promise<void>;
  now?: () => number;
}): Promise<CodingSessionGrantFailure | null> {
  const sleep =
    input.sleep ??
    ((milliseconds: number) =>
      new Promise<void>((resolve) => {
        globalThis.setTimeout(resolve, milliseconds);
      }));
  const now = input.now ?? (() => Date.now());
  const startedAt = now();
  for (let attempt = 1; ; attempt += 1) {
    try {
      await input.grant();
      return null;
    } catch (error) {
      const reason = codingSessionGrantFailureReason(error);
      if (
        !isCodingSessionGrantRateLimited(error) ||
        attempt >= CODING_SESSION_GRANT_MAX_ATTEMPTS
      ) {
        return { reason, attempts: attempt, waitBudgetSpent: false };
      }
      const remainingMs =
        CODING_SESSION_GRANT_WAIT_BUDGET_MS - (now() - startedAt);
      const delayMs = codingSessionGrantRetryDelayMs(
        reason,
        attempt,
        remainingMs,
      );
      if (delayMs <= 0) {
        return { reason, attempts: attempt, waitBudgetSpent: true };
      }
      await sleep(delayMs);
    }
  }
}
