/**
 * Pure helpers for the relay reconnect policy.
 *
 * Extracted from `RelayClient` so the decision rules — when to schedule a
 * reconnect, when to refuse to connect — live in one legible place that
 * unit tests can reach without booting the WS layer.
 *
 * The rules:
 *
 *  - **Terminal sessions never reconnect.** When the relay has explicitly
 *    rejected us (today: kind:22242 AUTH OK=false) the session is dead
 *    until the user re-engages (community switch or explicit preconnect).
 *    This guards the reconnect-timer catch handler — and the retry wrappers
 *    in `publishEvent` / `sendRawWithReconnectRetry` — from racing the
 *    `disconnected` state back to `reconnecting`.
 *
 *  - **No-op when a reconnect is already scheduled or in progress.**
 *    A pending timer or a live `wsId` means we have nothing to do.
 *
 *  - **No reconnect needed when nothing wants the socket.** No live
 *    subscription, no `keepAliveRequested` from `preconnect()` → don't
 *    keep an idle socket.
 */
export type RelayReconnectInputs = {
  terminal: boolean;
  hasPendingReconnect: boolean;
  hasLiveSocket: boolean;
  keepAliveRequested: boolean;
  hasLiveSubscriptions: boolean;
};

export function shouldScheduleReconnect(inputs: RelayReconnectInputs): boolean {
  if (inputs.terminal) return false;
  if (inputs.hasPendingReconnect) return false;
  if (inputs.hasLiveSocket) return false;
  if (!inputs.keepAliveRequested && !inputs.hasLiveSubscriptions) return false;
  return true;
}

export function shouldWaitForScheduledReconnect(inputs: {
  hasPendingReconnect: boolean;
}): boolean {
  return inputs.hasPendingReconnect;
}

/**
 * Reconnect delay with ±25 % jitter, capped at `maxDelayMs`.
 *
 * Spreads a fleet's AUTH storms across a 50 % window instead of hitting the
 * relay at the same instant — two devices on one key that lost the relay
 * together must not come back together. `random` is injectable for tests and
 * is expected in [0, 1).
 */
export function jitteredReconnectDelayMs(
  baseDelayMs: number,
  maxDelayMs: number,
  random: () => number = Math.random,
): number {
  const jittered = baseDelayMs * (0.75 + random() * 0.5);
  return Math.min(jittered, maxDelayMs);
}

/** Whether `ensureConnected()` should refuse with a terminal error. */
export function shouldRefuseConnect(inputs: { terminal: boolean }): boolean {
  return inputs.terminal;
}

export function isWebSocketClose(
  message: unknown,
): message is { type: "Close"; data?: unknown } {
  return (
    typeof message === "object" &&
    message !== null &&
    "type" in message &&
    message.type === "Close"
  );
}

export function isServiceRestartClose(message: unknown): boolean {
  if (!isWebSocketClose(message)) return false;
  if (!("data" in message)) return false;
  const data = message.data;
  return (
    typeof data === "object" &&
    data !== null &&
    "code" in data &&
    data.code === 1012
  );
}

/** Whether a WS-layer message is the plugin's `Error` frame. */
export function isWebSocketError(message: unknown): boolean {
  return (
    typeof message === "object" &&
    message !== null &&
    "type" in message &&
    message.type === "Error"
  );
}
