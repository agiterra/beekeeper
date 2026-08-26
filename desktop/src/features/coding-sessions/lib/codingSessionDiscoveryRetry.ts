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
      // Back-pressure is a server instruction, not a transport failure, so it
      // does not spend the bounded budget — it only advances the backoff
      // exponent (capped at 30s). See `isCodingSessionRelayBackPressure`.
      const willRetry =
        isCodingSessionRelayBackPressure(error) ||
        (attempt <= maxRetries &&
          isRetryableCodingSessionDiscoveryError(error));
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
 * Relay back-pressure: a "come back later", never a permanent failure.
 *
 * The relay closes a REQ with one of three distinct `rate-limited:` reasons
 * (`buzz-relay/src/connection.rs`):
 *
 * - `rate-limited: quota exceeded; retry in Ns` — the per-pubkey WebSocket
 *   admission budget (`human_ws_events_per_sec` × a 5s burst window, so 50
 *   REQ/EVENT frames by default) is spent.
 * - `rate-limited: too many concurrent requests` — the handler semaphore is
 *   saturated.
 * - `rate-limited: shared admission unavailable` — the shared (Redis) limiter
 *   could not be consulted, so the relay fails closed.
 *
 * All three are transient and all three must be retried. Matching only the
 * first was the defect behind "the session stops rendering when I leave and
 * come back": entering a coding-session view issues a burst of REQs (trusted
 * ingress history + live, create observations history + live, the conversation
 * lane, plus the channel window and its aux backfills), and the frames that
 * lose the quota race come back as one of the two unmatched reasons. The
 * discovery controller then treated them as fatal, leaving the catalog with no
 * entries and `isLoading` false — which the workspace renders as "Generation
 * not found" for a session that exists and is still streaming.
 */
export function isCodingSessionRelayBackPressure(error: unknown): boolean {
  const message = (
    error instanceof Error ? error.message : String(error)
  ).toLowerCase();
  return (
    message.includes("rate-limited:") || parseRateLimitHint(message) !== null
  );
}

/**
 * Retry relay transport/back-pressure failures, never arbitrary parsing,
 * authority, or invalid-filter errors.
 *
 * The last three fragments are the cold-start shapes. A read armed before the
 * socket is up fails with "Relay socket is not connected."; one that waits on
 * the reconnect coordinator's scheduled attempt fails with "Relay reconnect
 * failed."; a connect that throws surfaces "Failed to connect to relay.". None
 * of them says anything about the *filter* — they say the app asked a beat too
 * early — so treating them as fatal spent the whole projection on one unlucky
 * boot. `relay session is terminal` is deliberately absent: that latch is
 * cleared only by explicit re-engagement, and the connect it produces re-arms
 * the read through {@link armCodingSessionDiscoveryOnConnect} instead.
 */
export function isRetryableCodingSessionDiscoveryError(
  error: unknown,
): boolean {
  const message = (
    error instanceof Error ? error.message : String(error)
  ).toLowerCase();
  if (isCodingSessionRelayBackPressure(error)) {
    return true;
  }
  return [
    "timed out while loading",
    "relay disconnected",
    "socket closed",
    "connection reset",
    "failed to request channel history",
    "network error",
    "relay socket is not connected",
    "relay reconnect failed",
    "failed to connect to relay",
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
