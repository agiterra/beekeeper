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
 * up as a signed fact. It renders as its own clearly-local row and disappears
 * the moment the provider's verified echo arrives to replace it.
 *
 * Like {@link ./codingSessionPendingLifecycle}, this is not persistence: it
 * survives neither reload nor community switch (reset via
 * `resetPendingCodingSessionTurns`), and an unanswered record expires after
 * {@link PENDING_CODING_SESSION_TURN_TTL_MS} so a provider that never answers
 * cannot leave a row claiming a turn is still in flight an hour later. A
 * record the provider has signed for (`turn_queued`, `turn_degraded`,
 * `turn_injected`, `turn_delivery_unknown`) is exempt from that expiry and
 * ages visibly instead — the relay is the mailbox, so a turn waiting behind
 * an hour of work is still coming, and a delivery the provider could not
 * establish is the person's to settle, not a clock's.
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
  /**
   * The words the person actually typed, when they differ from {@link text}.
   *
   * The umbrella composer strips a routing `@handle` before publishing, so the
   * wire text above is not what belongs back in the editor if this turn is
   * refused or dropped. Carried on the row rather than only in the watcher's
   * memory because the watcher is a component: it dies when the composer
   * unmounts, and a composer that remounts has to be able to re-arm a watch
   * for a turn the provider is still holding.
   */
  draft?: string;
  /** This client's signer, which the provider stamps onto its echo. */
  operatorPubkey: string | null;
  recordedAt: number;
  /** Set once the relay has accepted the command. */
  published: boolean;
  /**
   * Set once the provider's own signed `turn_queued` receipt names this
   * command. The turn is in the provider's mailbox behind whatever it is
   * already doing — accepted, not started, and certainly not "thinking".
   */
  queuedByProvider?: boolean;
  /**
   * Set once the provider's own signed `turn_degraded` receipt names this
   * command: the sender asked to steer the running turn and this execution's
   * runtime cannot, so the turn waits for the next boundary instead. The turn
   * is not lost and was not merged into the running one — but it is not what
   * was asked for, so the row says so rather than reading as an ordinary queue.
   */
  degradedByProvider?: boolean;
  /**
   * Set once the provider's own signed `turn_injected` receipt names this
   * command: the runtime acknowledged the input as joined into the turn that
   * was already running. Not a new turn — the row still settles on the
   * `user_prompt{steered: true}` echo whose `commandId` matches, exactly as a
   * started turn does — but it says so, because "queued" or silence would
   * both misreport what happened to the person's correction.
   */
  injectedByProvider?: boolean;
  /**
   * Set once the provider's own signed `turn_delivery_unknown` receipt names
   * this command, with the code and words it carried. Terminal: the provider
   * will not replay the input, and this client will not either — the words
   * may already be inside the running turn. The row stays, says so, and
   * offers dismissal; it never retires itself and never restores the draft
   * as if the turn had been refused. A later `turn_injected` for the same
   * command (a reconciled attempt) outranks it.
   */
  deliveryUnknown?: { code: string; message: string };
};

/**
 * True once the provider has signed for this turn — queued, degraded to the
 * boundary, injected into the running turn, or answered delivery-unknown.
 * All mean the same thing for the row's lifetime: the provider has spoken
 * for it, and the client is no longer the only thing claiming it exists, so
 * the unanswered-row clock does not apply.
 */
export function pendingCodingSessionTurnHeldByProvider(
  pending: PendingCodingSessionTurn,
): boolean {
  return (
    pending.queuedByProvider === true ||
    pending.degradedByProvider === true ||
    pending.injectedByProvider === true ||
    pending.deliveryUnknown !== undefined
  );
}

/**
 * When a published turn stops reading as in-flight.
 *
 * Long enough that the ordinary case never gains a caption, but short enough
 * that a provider which never picks the turn up does not look normal.
 */
export const PENDING_CODING_SESSION_TURN_STALL_MS = 10_000;

/**
 * When an *unanswered* pending row is dropped outright.
 *
 * A row that has gone unanswered this long is not going to be answered — the
 * command was addressed to a target no live provider owns — and keeping it
 * forever would put a permanent unsent message above the composer. The stall
 * label above is what warns the person in time to keep their words; this is the
 * backstop, not the disclosure.
 *
 * A row the provider has signed for is exempt: a turn queued behind a long
 * turn is *supposed* to sit there, and dropping it at three minutes would
 * delete a message that is still coming. Those rows age out loud instead —
 * see {@link pendingCodingSessionTurnHeldByProvider}.
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

/**
 * Record the provider's signed `turn_queued` for a turn this client sent.
 *
 * Kept separate from `published`: the relay accepting the command and the
 * provider accepting the turn are two different facts, and the row says which
 * one it has.
 */
export function markPendingCodingSessionTurnQueued(
  channelId: string,
  commandId: string,
): void {
  const key = pendingCodingSessionTurnKey({ channelId, commandId });
  let changed = false;
  const next = pendingTurns.map((entry) => {
    if (
      pendingCodingSessionTurnKey(entry) !== key ||
      entry.queuedByProvider === true
    ) {
      return entry;
    }
    changed = true;
    return { ...entry, published: true, queuedByProvider: true };
  });
  if (!changed) return;
  pendingTurns = next;
  notify();
}

/**
 * Record the provider's signed `turn_degraded` for a turn this client sent.
 *
 * Kept apart from `queuedByProvider` because it is a different sentence: the
 * turn is queued *because the steer could not happen*, and a row that only
 * said "queued" would quietly swallow the fact that the person's mid-turn
 * correction is not reaching the running turn.
 */
export function markPendingCodingSessionTurnDegraded(
  channelId: string,
  commandId: string,
): void {
  const key = pendingCodingSessionTurnKey({ channelId, commandId });
  let changed = false;
  const next = pendingTurns.map((entry) => {
    if (
      pendingCodingSessionTurnKey(entry) !== key ||
      entry.degradedByProvider === true
    ) {
      return entry;
    }
    changed = true;
    return { ...entry, published: true, degradedByProvider: true };
  });
  if (!changed) return;
  pendingTurns = next;
  notify();
}

/**
 * Record the provider's signed `turn_injected` for a turn this client sent.
 *
 * The native steer landed. The row is not retired here — the provider's
 * `user_prompt{steered: true}` echo is what retires it, by `commandId`, like
 * every other row — but it stops reading as queued or waiting.
 */
export function markPendingCodingSessionTurnInjected(
  channelId: string,
  commandId: string,
): void {
  const key = pendingCodingSessionTurnKey({ channelId, commandId });
  let changed = false;
  const next = pendingTurns.map((entry) => {
    if (
      pendingCodingSessionTurnKey(entry) !== key ||
      entry.injectedByProvider === true
    ) {
      return entry;
    }
    changed = true;
    return { ...entry, published: true, injectedByProvider: true };
  });
  if (!changed) return;
  pendingTurns = next;
  notify();
}

/**
 * Record the provider's signed `turn_delivery_unknown` for a turn this client
 * sent.
 *
 * Idempotent on the first answer: a replayed receipt is the same fact. The
 * row keeps the words, is exempt from expiry, and leaves the list only when
 * the person dismisses it ({@link forgetPendingCodingSessionTurn}) or — for
 * the input that did land after all — when the steered echo names it.
 */
export function markPendingCodingSessionTurnDeliveryUnknown(
  channelId: string,
  commandId: string,
  outcome: { code: string; message: string },
): void {
  const key = pendingCodingSessionTurnKey({ channelId, commandId });
  let changed = false;
  const next = pendingTurns.map((entry) => {
    if (
      pendingCodingSessionTurnKey(entry) !== key ||
      entry.deliveryUnknown !== undefined
    ) {
      return entry;
    }
    changed = true;
    return {
      ...entry,
      published: true,
      deliveryUnknown: { code: outcome.code, message: outcome.message },
    };
  });
  if (!changed) return;
  pendingTurns = next;
  notify();
}

/**
 * Every row for one execution that the provider has signed for and this client
 * is still showing.
 *
 * The re-arm list. A held row is exempt from the pending TTL and from the
 * refusal watch's deadline — deliberately, because a turn queued behind an
 * hour of work is still coming — which leaves the watch as the only thing that
 * can ever retire it. The watch is a component, so it dies on unmount while
 * the row survives; a composer coming back has to pick these up again or the
 * turn's terminal receipt lands on nobody and the row (with the person's words
 * in it) stays on screen forever.
 */
export function heldPendingCodingSessionTurns(
  channelId: string,
  targetKey: string,
): readonly PendingCodingSessionTurn[] {
  return pendingTurns.filter(
    (entry) =>
      entry.channelId === channelId &&
      entry.targetKey === targetKey &&
      pendingCodingSessionTurnHeldByProvider(entry),
  );
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

/**
 * How many text-only settlements are remembered per community.
 *
 * The disclosure is per message and only matters while that message is on
 * screen, so this is small on purpose — it exists to keep a long session from
 * accumulating an unbounded set.
 */
export const MAX_TEXT_SETTLED_CODING_SESSION_ECHOES = 64;

const NO_TEXT_SETTLED_ECHOES: ReadonlySet<string> = new Set();
let textSettledEchoIds: ReadonlySet<string> = NO_TEXT_SETTLED_ECHOES;

/**
 * Remember which verified echoes were matched to a sent turn by their words
 * alone, so the transcript can say so on the message itself.
 *
 * This is the honesty half of the text fallback. A row settled by command id
 * is a fact the provider stated; a row settled by text is this client's best
 * guess about which of possibly identical messages it is looking at, and the
 * message it retired should not pretend otherwise.
 */
export function noteTextSettledCodingSessionEchoes(
  echoIds: readonly string[],
): void {
  const additions = echoIds.filter((id) => !textSettledEchoIds.has(id));
  if (additions.length === 0) return;
  const next = [...textSettledEchoIds, ...additions];
  textSettledEchoIds = new Set(
    next.slice(
      Math.max(0, next.length - MAX_TEXT_SETTLED_CODING_SESSION_ECHOES),
    ),
  );
  notify();
}

/** The echo ids settled by text, for tests and diagnostics. */
export function readTextSettledCodingSessionEchoes(): ReadonlySet<string> {
  return textSettledEchoIds;
}

/** The echo ids settled by text, as a stable subscription. */
export function useTextSettledCodingSessionEchoes(): ReadonlySet<string> {
  return React.useSyncExternalStore(
    subscribe,
    readTextSettledCodingSessionEchoes,
    readTextSettledCodingSessionEchoes,
  );
}

/** Community switch teardown — see `resetCommunityState()`. */
export function resetPendingCodingSessionTurns(): void {
  const hadTextSettled = textSettledEchoIds.size > 0;
  if (pendingTurns.length === 0 && !hadTextSettled) return;
  pendingTurns = [];
  textSettledEchoIds = NO_TEXT_SETTLED_ECHOES;
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
  /** The projected item's own id, used to disclose a text-only settlement. */
  id?: string;
  type?: string;
  role?: string;
  text?: string;
  operatorPubkey?: string | null;
  /**
   * The 44220 command id the provider stamped on its echo, when it stamps
   * one. This is the join; the text match below is only what is left when a
   * provider predates the field.
   */
  commandId?: string;
};

/** What a pending row should say about itself right now. */
export type PendingCodingSessionTurnState =
  | "sending"
  | "unknown"
  | "injected"
  | "degraded"
  | "queued"
  | "waiting"
  | "stalled";

/** Classify a pending turn by what has actually happened to it. */
export function pendingCodingSessionTurnState(
  pending: PendingCodingSessionTurn,
  now: number,
): PendingCodingSessionTurnState {
  if (!pending.published) return "sending";
  // A signed receipt outranks the stall clock, because it answers the exact
  // question the stall label exists to raise: something did pick this turn up.
  // It is waiting behind other work, and saying so beats both silence and
  // "not picked up yet". Injected outranks everything, including a
  // delivery-unknown answer that preceded it: a late acknowledgement can
  // reconcile an unknown attempt as injected, and that later, definite fact
  // is the one the row reads (the steered echo then retires it as usual).
  // Delivery-unknown outranks the rest as the provider's terminal word on
  // its own. Degraded outranks queued: a provider that could not steer
  // publishes both, and the half the person did not ask for is the half
  // worth reading.
  if (pending.injectedByProvider === true) return "injected";
  if (pending.deliveryUnknown !== undefined) return "unknown";
  if (pending.degradedByProvider === true) return "degraded";
  if (pending.queuedByProvider === true) return "queued";
  return now - pending.recordedAt > PENDING_CODING_SESSION_TURN_STALL_MS
    ? "stalled"
    : "waiting";
}

/**
 * How long a held row has been waiting, in the coarsest unit that is still
 * true. Seconds matter here — the stall clock fires ten seconds in, and
 * "just now" at that point would be an evasion rather than a rounding.
 */
export function formatPendingCodingSessionTurnAge(ageMs: number): string {
  const seconds = Math.max(0, Math.floor(ageMs / 1_000));
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3_600) return `${Math.floor(seconds / 60)}m`;
  return `${Math.floor(seconds / 3_600)}h`;
}

/** How a pending row was retired, for callers that must disclose the join. */
export type PendingCodingSessionTurnSettlement = {
  key: string;
  commandId: string;
  /**
   * `"commandId"` — the provider named the command this echo answers.
   * `"text"` — it did not, and the words were the only thing left to match on.
   */
  by: "commandId" | "text";
  /** The echo's own item id, when it had one. */
  echoId?: string;
};

/**
 * Decide which pending turns a surface should still show for one execution.
 *
 * A pending turn is retired when the provider's verified prompt echo for it
 * appears in that execution's transcript, or when its TTL runs out — the TTL
 * applying only to a turn no provider has signed for. There are two joins and
 * they are not equal:
 *
 * 1. **The command id.** A provider that stamps `commandId` on its
 *    `user_prompt` has told us exactly which sent turn this echo answers.
 *    Sending the same sentence twice settles each row against its own echo,
 *    in whatever order they arrive.
 * 2. **The text, as a fallback only.** A provider from before that field
 *    exists says nothing about which command it is echoing, so the words plus
 *    the operator are all there is. That is a *guess*: it cannot tell two
 *    identical sentences apart except by arrival order, and a settlement made
 *    this way is reported back in {@link PendingCodingSessionTurnSettlement}
 *    so the surface can say the row was matched by text rather than named.
 *
 * An echo that carries a command id is never text-matched — a different
 * command's echo must not retire this row just because the words agree.
 *
 * Pure — the caller clears `consumedKeys` in an effect.
 */
export function resolvePendingCodingSessionTurns(
  pending: readonly PendingCodingSessionTurn[],
  scope: { channelId: string; targetKey: string },
  echoes: readonly PendingCodingSessionTurnEcho[],
  now: number,
): {
  visible: PendingCodingSessionTurn[];
  consumedKeys: string[];
  settlements: PendingCodingSessionTurnSettlement[];
} {
  const mine = pending.filter(
    (entry) =>
      entry.channelId === scope.channelId &&
      entry.targetKey === scope.targetKey,
  );
  if (mine.length === 0) {
    return { visible: [], consumedKeys: [], settlements: [] };
  }

  // One echo retires one pending turn: sending the same words twice on purpose
  // is a real thing people do, and the second row must survive the first echo.
  const unmatched = echoes.filter(isPromptEcho);
  const consumedKeys: string[] = [];
  const settlements: PendingCodingSessionTurnSettlement[] = [];
  const expired = new Set<string>();
  const settled = new Set<string>();

  for (const entry of mine) {
    if (pendingCodingSessionTurnHeldByProvider(entry)) continue;
    if (now - entry.recordedAt > PENDING_CODING_SESSION_TURN_TTL_MS) {
      const key = pendingCodingSessionTurnKey(entry);
      expired.add(key);
      consumedKeys.push(key);
    }
  }

  // Pass one: named joins, for every row, before any guess is allowed to
  // consume an echo a later row could have claimed by name.
  for (const entry of mine) {
    const key = pendingCodingSessionTurnKey(entry);
    if (expired.has(key)) continue;
    const index = unmatched.findIndex(
      (echo) =>
        echo.commandId !== undefined && echo.commandId === entry.commandId,
    );
    if (index < 0) continue;
    const [echo] = unmatched.splice(index, 1);
    settled.add(key);
    consumedKeys.push(key);
    settlements.push({
      key,
      commandId: entry.commandId,
      by: "commandId",
      ...(typeof echo.id === "string" ? { echoId: echo.id } : {}),
    });
  }

  // Pass two: the fallback, over echoes that named no command at all.
  const visible: PendingCodingSessionTurn[] = [];
  for (const entry of mine) {
    const key = pendingCodingSessionTurnKey(entry);
    if (expired.has(key) || settled.has(key)) continue;
    const index = unmatched.findIndex(
      (echo) => echo.commandId === undefined && echoMatches(echo, entry),
    );
    if (index < 0) {
      visible.push(entry);
      continue;
    }
    const [echo] = unmatched.splice(index, 1);
    consumedKeys.push(key);
    settlements.push({
      key,
      commandId: entry.commandId,
      by: "text",
      ...(typeof echo.id === "string" ? { echoId: echo.id } : {}),
    });
  }
  return { visible, consumedKeys, settlements };
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
