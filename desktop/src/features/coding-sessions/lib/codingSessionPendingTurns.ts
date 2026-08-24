/**
 * The turn you just sent, on screen before anyone else has said anything.
 *
 * A coding-session turn is a signed kind-44220 command, and the transcript is
 * fact-driven: the words only appear once the provider publishes its own signed
 * `user_prompt` item (44225) and the client verifies it. That is two relay hops
 * and a provider round of work after the person pressed Enter, so the message
 * they just wrote was simply absent for as long as it took.
 *
 * This store closes that gap without touching the trust boundary. A pending
 * turn is never merged into a catalog record's transcript and is never dressed
 * up as a signed fact — it renders as its own clearly-local row that says what
 * is actually true at each step ("Sending…", then waiting on the provider), and
 * disappears the moment the provider's verified echo arrives to replace it.
 *
 * Like {@link ./codingSessionPendingLifecycle}, this is not persistence: it
 * survives neither reload nor community switch (reset via
 * `resetPendingCodingSessionTurns`), and every record expires after
 * {@link PENDING_CODING_SESSION_TURN_TTL_MS} so a provider that never answers
 * cannot leave a row claiming a turn is still in flight an hour later.
 */
import * as React from "react";

/** One turn published from this client and not yet echoed by its provider. */
export type PendingCodingSessionTurn = {
  channelId: string;
  /** `buildCodingSessionTargetKey` of the execution the turn was sent to. */
  targetKey: string;
  /** The 44220's command id — the key a refusal receipt is addressed by. */
  commandId: string;
  /**
   * Exactly the text that was published, which is what the provider echoes.
   * The umbrella composer strips a routing `@handle` before publishing, so
   * this is deliberately the wire text and not the raw draft.
   */
  text: string;
  /** This client's signer, which the provider stamps onto its echo. */
  operatorPubkey: string | null;
  recordedAt: number;
  /** Set once the relay has accepted the command. */
  published: boolean;
};

/**
 * When a published turn stops reading as in-flight.
 *
 * Matched to `CODING_SESSION_TURN_REFUSAL_DEADLINE_MS`: past that point the
 * provider has neither refused the turn nor begun it, and a spinner would be
 * claiming progress nobody has evidence of.
 */
export const PENDING_CODING_SESSION_TURN_STALL_MS = 20_000;

/**
 * When a pending row is dropped outright.
 *
 * A row that has gone unanswered this long is not going to be answered — the
 * command was addressed to a target no live provider owns — and keeping it
 * forever would put a permanent unsent message above the composer. The stall
 * label above is what warns the person in time to keep their words; this is the
 * backstop, not the disclosure.
 */
export const PENDING_CODING_SESSION_TURN_TTL_MS = 3 * 60_000;

/**
 * How many pending turns are kept per execution. Sending faster than a provider
 * can echo is possible; growing this list without bound is not useful.
 */
export const MAX_PENDING_CODING_SESSION_TURNS_PER_TARGET = 8;

/** Stable identity of a pending turn. */
export function pendingCodingSessionTurnKey(
  pending: Pick<PendingCodingSessionTurn, "channelId" | "commandId">,
): string {
  return `turn:${pending.channelId}:${pending.commandId}`;
}

let pendingTurns: readonly PendingCodingSessionTurn[] = [];
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

/**
 * Record a turn as sent. Called before the publish resolves — the point of the
 * row is to exist during that wait — so it starts `published: false`.
 */
export function recordPendingCodingSessionTurn(
  pending: PendingCodingSessionTurn,
): void {
  const key = pendingCodingSessionTurnKey(pending);
  const others = pendingTurns.filter(
    (entry) => pendingCodingSessionTurnKey(entry) !== key,
  );
  const sameTarget = others.filter(
    (entry) =>
      entry.channelId === pending.channelId &&
      entry.targetKey === pending.targetKey,
  );
  const overflow = Math.max(
    0,
    sameTarget.length + 1 - MAX_PENDING_CODING_SESSION_TURNS_PER_TARGET,
  );
  const dropped = new Set(
    sameTarget.slice(0, overflow).map(pendingCodingSessionTurnKey),
  );
  pendingTurns = [
    ...others.filter(
      (entry) => !dropped.has(pendingCodingSessionTurnKey(entry)),
    ),
    pending,
  ];
  notify();
}

/** Mark a recorded turn as accepted by the relay. */
export function markPendingCodingSessionTurnPublished(
  channelId: string,
  commandId: string,
): void {
  const key = pendingCodingSessionTurnKey({ channelId, commandId });
  let changed = false;
  const next = pendingTurns.map((entry) => {
    if (pendingCodingSessionTurnKey(entry) !== key || entry.published) {
      return entry;
    }
    changed = true;
    return { ...entry, published: true };
  });
  if (!changed) return;
  pendingTurns = next;
  notify();
}

/** Drop the given records — echoed by the provider, refused, or expired. */
export function clearPendingCodingSessionTurns(keys: readonly string[]): void {
  if (keys.length === 0) return;
  const drop = new Set(keys);
  const next = pendingTurns.filter(
    (entry) => !drop.has(pendingCodingSessionTurnKey(entry)),
  );
  if (next.length === pendingTurns.length) return;
  pendingTurns = next;
  notify();
}

/** Drop one record by the coordinates its caller already holds. */
export function forgetPendingCodingSessionTurn(
  channelId: string,
  commandId: string,
): void {
  clearPendingCodingSessionTurns([
    pendingCodingSessionTurnKey({ channelId, commandId }),
  ]);
}

/** Community switch teardown — see `resetCommunityState()`. */
export function resetPendingCodingSessionTurns(): void {
  if (pendingTurns.length === 0) return;
  pendingTurns = [];
  notify();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Every pending turn this client is holding, for tests and diagnostics. */
export function readPendingCodingSessionTurns(): readonly PendingCodingSessionTurn[] {
  return pendingTurns;
}

/** Every pending turn this client is holding. */
export function usePendingCodingSessionTurns(): readonly PendingCodingSessionTurn[] {
  return React.useSyncExternalStore(
    subscribe,
    readPendingCodingSessionTurns,
    readPendingCodingSessionTurns,
  );
}

/**
 * The slice of a projected transcript item that can retire a pending turn.
 * Structural so this module stays free of the renderer's types.
 */
export type PendingCodingSessionTurnEcho = {
  type?: string;
  role?: string;
  text?: string;
  operatorPubkey?: string | null;
};

/** What a pending row should say about itself right now. */
export type PendingCodingSessionTurnState = "sending" | "waiting" | "stalled";

/** Classify a pending turn by what has actually happened to it. */
export function pendingCodingSessionTurnState(
  pending: PendingCodingSessionTurn,
  now: number,
): PendingCodingSessionTurnState {
  if (!pending.published) return "sending";
  return now - pending.recordedAt > PENDING_CODING_SESSION_TURN_STALL_MS
    ? "stalled"
    : "waiting";
}

/**
 * Decide which pending turns a surface should still show for one execution.
 *
 * A pending turn is retired when the provider's verified prompt echo carrying
 * the same text appears in that execution's transcript, or when its TTL runs
 * out. Matching is on the published text plus, when both sides know it, the
 * operator: the provider's `user_prompt` item carries no command id, so text is
 * the only join available, and two different people sending the same words to
 * the same execution must not retire each other's rows.
 *
 * Pure — the caller clears `consumedKeys` in an effect.
 */
export function resolvePendingCodingSessionTurns(
  pending: readonly PendingCodingSessionTurn[],
  scope: { channelId: string; targetKey: string },
  echoes: readonly PendingCodingSessionTurnEcho[],
  now: number,
): { visible: PendingCodingSessionTurn[]; consumedKeys: string[] } {
  const mine = pending.filter(
    (entry) =>
      entry.channelId === scope.channelId &&
      entry.targetKey === scope.targetKey,
  );
  if (mine.length === 0) return { visible: [], consumedKeys: [] };

  // One echo retires one pending turn: sending the same words twice on purpose
  // is a real thing people do, and the second row must survive the first echo.
  const unmatched = echoes.filter(isPromptEcho);
  const visible: PendingCodingSessionTurn[] = [];
  const consumedKeys: string[] = [];
  for (const entry of mine) {
    if (now - entry.recordedAt > PENDING_CODING_SESSION_TURN_TTL_MS) {
      consumedKeys.push(pendingCodingSessionTurnKey(entry));
      continue;
    }
    const index = unmatched.findIndex((echo) => echoMatches(echo, entry));
    if (index >= 0) {
      unmatched.splice(index, 1);
      consumedKeys.push(pendingCodingSessionTurnKey(entry));
      continue;
    }
    visible.push(entry);
  }
  return { visible, consumedKeys };
}

function isPromptEcho(echo: PendingCodingSessionTurnEcho): boolean {
  return echo.type === "message" && echo.role === "user";
}

function echoMatches(
  echo: PendingCodingSessionTurnEcho,
  pending: PendingCodingSessionTurn,
): boolean {
  if ((echo.text ?? "") !== pending.text) return false;
  const echoOperator = echo.operatorPubkey ?? null;
  if (echoOperator === null || pending.operatorPubkey === null) return true;
  return echoOperator.toLowerCase() === pending.operatorPubkey.toLowerCase();
}
