import {
  MAX_HINT_SECONDS,
  parseRateLimitHint,
  rateLimitRemainingMs,
} from "@/shared/api/relayRateLimitGate";

const RETRY_BASE_DELAY_MS = 1_000;
const RETRY_MAX_BACKOFF_MS = 30_000;
const RETRY_JITTER_WINDOW_MS = 750;
const RETRY_MAX_DELAY_MS = MAX_HINT_SECONDS * 1_000;

type RetryTimer = ReturnType<typeof window.setTimeout>;

export type CodingSessionDiscoveryRetry = {
  attempt: number;
  delayMs: number;
  willRetry: boolean;
};

export type CodingSessionDiscoveryController = {
  /**
   * Start a refresh unless one is already running or waiting to retry.
   *
   * Returning false means the request was coalesced into the existing work.
   */
  request: () => boolean;
  cancel: () => void;
};

type CodingSessionDiscoveryControllerOptions<Result> = {
  load: () => Promise<Result>;
  onAttemptStart: () => void;
  onSuccess: (result: Result) => void;
  onError: (error: unknown, retry: CodingSessionDiscoveryRetry) => void;
  retrySeed: string;
  maxRetries?: number;
  getRateLimitRemainingMs?: () => number;
  schedule?: (callback: () => void, delayMs: number) => RetryTimer;
  clearSchedule?: (timer: RetryTimer) => void;
};

/**
 * Keep a session-history refresh alive across relay back-pressure.
 *
 * Only one request may be active or scheduled at a time. Reconnect callbacks
 * that arrive while either state is active are deliberately coalesced, which
 * prevents a cold app from multiplying the same history REQ while the relay is
 * already asking clients to slow down.
 */
export function createCodingSessionDiscoveryController<Result>({
  load,
  onAttemptStart,
  onSuccess,
  onError,
  retrySeed,
  maxRetries = 3,
  getRateLimitRemainingMs = rateLimitRemainingMs,
  schedule = (callback, delayMs) => window.setTimeout(callback, delayMs),
  clearSchedule = (timer) => window.clearTimeout(timer),
}: CodingSessionDiscoveryControllerOptions<Result>): CodingSessionDiscoveryController {
  let cancelled = false;
  let inFlight = false;
  let retryTimer: RetryTimer | null = null;
  let retryAttempt = 0;

  const run = async () => {
    if (cancelled || inFlight || retryTimer !== null) return;
    inFlight = true;
    onAttemptStart();
    try {
      const result = await load();
      if (cancelled) return;
      retryAttempt = 0;
      onSuccess(result);
    } catch (error) {
      if (cancelled) return;
      const delayMs = codingSessionDiscoveryRetryDelayMs(
        error,
        retryAttempt,
        retrySeed,
        getRateLimitRemainingMs(),
      );
      const attempt = retryAttempt + 1;
      const willRetry =
        attempt <= maxRetries && isRetryableCodingSessionDiscoveryError(error);
      const retry = { attempt, delayMs, willRetry };
      retryAttempt = willRetry ? Math.min(retryAttempt + 1, 31) : 0;
      onError(error, retry);
      if (willRetry) {
        retryTimer = schedule(() => {
          retryTimer = null;
          void run();
        }, delayMs);
      }
    } finally {
      inFlight = false;
    }
  };

  return {
    request() {
      if (cancelled || inFlight || retryTimer !== null) return false;
      void run();
      return true;
    },
    cancel() {
      cancelled = true;
      if (retryTimer !== null) {
        clearSchedule(retryTimer);
        retryTimer = null;
      }
    },
  };
}

/**
 * Retry relay transport/back-pressure failures, never arbitrary parsing,
 * authority, or invalid-filter errors.
 */
export function isRetryableCodingSessionDiscoveryError(
  error: unknown,
): boolean {
  const message = (
    error instanceof Error ? error.message : String(error)
  ).toLowerCase();
  if (
    message.includes("rate-limited:") &&
    (message.includes("quota exceeded") || parseRateLimitHint(message) !== null)
  ) {
    return true;
  }
  return [
    "timed out while loading",
    "relay disconnected",
    "socket closed",
    "connection reset",
    "failed to request channel history",
    "network error",
  ].some((fragment) => message.includes(fragment));
}

/** Server-guided exponential backoff with deterministic per-scope jitter. */
export function codingSessionDiscoveryRetryDelayMs(
  error: unknown,
  attempt: number,
  retrySeed: string,
  activeGateRemainingMs: number,
): number {
  const message = error instanceof Error ? error.message : String(error);
  const hintSeconds = parseRateLimitHint(message);
  const hintMs =
    hintSeconds === null
      ? 0
      : Math.min(Math.max(hintSeconds, 0) * 1_000, RETRY_MAX_DELAY_MS);
  const gateMs = Math.min(
    Math.max(activeGateRemainingMs, 0),
    RETRY_MAX_DELAY_MS,
  );
  const exponent = Math.min(Math.max(attempt, 0), 30);
  const backoffMs = Math.min(
    RETRY_BASE_DELAY_MS * 2 ** exponent,
    RETRY_MAX_BACKOFF_MS,
  );
  const jitterMs =
    stableRetryHash(`${retrySeed}:${exponent}`) % RETRY_JITTER_WINDOW_MS;
  return Math.min(
    Math.max(backoffMs, hintMs, gateMs) + jitterMs,
    RETRY_MAX_DELAY_MS,
  );
}

function stableRetryHash(value: string): number {
  let hash = 2_166_136_261;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 16_777_619);
  }
  return hash >>> 0;
}
