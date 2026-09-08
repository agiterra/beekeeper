import 'dart:math';

import 'relay_closed_policy.dart';
import 'relay_rate_limit_gate.dart';

/// Base delay for the first retry of a `CLOSED` live subscription and for the
/// first reconnect attempt.
const relayBaseRetryDelayMs = 1000;

/// Ceiling for exponential retry and reconnect backoff.
const relayMaxRetryDelayMs = 30000;

/// Exponential backoff for retry [attempt] (0-based): 1 s, 2 s, 4 s … capped
/// at [relayMaxRetryDelayMs] from the fifth attempt on. Saturates before the
/// shift so a very high attempt count cannot overflow.
int closedRetryBackoffMs(int attempt) => attempt >= 5
    ? relayMaxRetryDelayMs
    : relayBaseRetryDelayMs * (1 << attempt);

/// Delay before retrying a live subscription the relay `CLOSED`.
///
/// A rate-limited close never retries before the shared gate reopens: the
/// delay is the larger of the exponential backoff and the gate's remaining
/// window ([gateRemainingMs]); when the gate reports no remaining time the
/// relay's own `retry in Ns` hint (or the gate default) is the floor instead.
int closedRetryDelayMs({
  required int attempt,
  required RelayClosedClass closedClass,
  required String message,
  required int gateRemainingMs,
}) {
  final backoffMs = closedRetryBackoffMs(attempt);
  if (closedClass != RelayClosedClass.rateLimited) return backoffMs;
  final retrySeconds = parseRateLimitRetrySeconds(message);
  final fallbackMs =
      (retrySeconds != null && retrySeconds > 0
          ? min(retrySeconds, RelayRateLimitGate.maxRetrySeconds)
          : RelayRateLimitGate.defaultRetrySeconds) *
      1000;
  return max(backoffMs, gateRemainingMs == 0 ? fallbackMs : gateRemainingMs);
}
