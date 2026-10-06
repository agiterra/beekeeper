import { KIND_AGENT_OBSERVER_FRAME } from "../constants/kinds";

/**
 * Proactive send budget for the relay WebSocket.
 *
 * The relay admits at most `RELAY_BURST_CAPACITY` EVENT/REQ/COUNT frames per
 * `BUDGET_WINDOW_MS` for one (community, pubkey) — shared by every device
 * signing with that key — and at most `RELAY_EVENTS_PER_MINUTE` EVENTs per
 * minute (`crates/beekeeper-relay/src/admission.rs`, `rejection.rs`). The reactive
 * gate (`relayRateLimitGate.ts`) only reacts *after* a refusal; this bucket
 * paces sends so the refusal does not happen in the first place, and keeps a
 * write reserve so a read storm (reconnect replay, catch-up fetches) can never
 * spend the frames a user's message needs.
 *
 * Lanes:
 * - `read`      — REQ / COUNT. Waits for a slot; never touches the reserve.
 * - `write`     — durable EVENT or observer control. Waits for a slot; may
 *                 use the full burst. Controls remain charged to both windows.
 * - `ephemeral` — kind 20000–29999 EVENT (typing, presence, terminal frames).
 *                 Counted by the relay exactly like a durable EVENT (Andy's
 *                 decision: no exemption), so it shares the minute counter but
 *                 stays out of the write reserve, except explicit observer
 *                 controls (Stop/model switch). Droppable senders call
 *                 `tryAcquire`; awaited senders (`publishEvent`) call `acquire`.
 * - `free`      — AUTH / CLOSE: the relay does not count them.
 *
 * Module singleton, reset on community switch through `resetRelaySendBudget()`
 * from `resetCommunityState()`.
 */

export type SendLane = "read" | "write" | "ephemeral" | "free";

/** Frames per window the relay admits for one key (`admission.rs`). */
export const RELAY_BURST_CAPACITY = 50;

/** Sliding window over which the burst capacity applies. */
export const BUDGET_WINDOW_MS = 5_000;

/** Durable + ephemeral EVENTs per minute the relay admits for one key. */
export const RELAY_EVENTS_PER_MINUTE = 60;

/** Window over which {@link RELAY_EVENTS_PER_MINUTE} applies. */
export const EVENT_MINUTE_WINDOW_MS = 60_000;

/**
 * How many devices this client assumes are sharing its key.
 *
 * The paired phone signs with the desktop's nsec, so both devices draw on one
 * per-key burst. Each takes half until the relay's per-connection budgets are
 * deployed; then this becomes 1 (one constant, one place — keep the mobile
 * client's `deviceShare` in step).
 */
export const DEVICE_SHARE = 2;

/** This device's share of the relay burst. */
export const LOCAL_BURST_CAPACITY = Math.floor(
  RELAY_BURST_CAPACITY / DEVICE_SHARE,
);

/**
 * Frames of the local burst — and EVENTs of the minute counter — held back
 * from reads and ephemerals so a durable write is always admissible.
 */
export const WRITE_RESERVE = 8;

/** This device's share of the per-key EVENT minute counter. */
export const LOCAL_EVENTS_PER_MINUTE = Math.floor(
  RELAY_EVENTS_PER_MINUTE / DEVICE_SHARE,
);

/** Kinds 20000–29999 are ephemeral per NIP-01. */
export function isEphemeralKind(kind: number): boolean {
  return kind >= 20_000 && kind < 30_000;
}

/**
 * Lane for one outbound NIP-01 client frame, keyed on `payload[0]`.
 *
 * Unknown frame types are treated as reads: the relay counts anything it does
 * not recognise as free only when it is AUTH or CLOSE, so charging is the
 * conservative default.
 */
export function classifySendLane(payload: readonly unknown[]): SendLane {
  const type = payload[0];
  if (type === "AUTH" || type === "CLOSE") return "free";
  if (type === "EVENT") {
    const event = payload[1];
    const kind =
      typeof event === "object" && event !== null && "kind" in event
        ? (event as { kind?: unknown }).kind
        : undefined;
    // A user control must not queue behind the read storm it may be trying
    // to stop. This is priority within the same limits, never a free frame.
    const tags =
      typeof event === "object" && event !== null && "tags" in event
        ? (event as { tags?: unknown }).tags
        : undefined;
    if (
      kind === KIND_AGENT_OBSERVER_FRAME &&
      Array.isArray(tags) &&
      tags.some(
        (tag) =>
          Array.isArray(tag) && tag[0] === "frame" && tag[1] === "control",
      )
    )
      return "write";
    return typeof kind === "number" && isEphemeralKind(kind)
      ? "ephemeral"
      : "write";
  }
  return "read";
}

export type SendBudgetOptions = {
  /** Local burst capacity per window (default {@link LOCAL_BURST_CAPACITY}). */
  capacity?: number;
  /** Burst window in ms (default {@link BUDGET_WINDOW_MS}). */
  windowMs?: number;
  /** Frames kept back for writes (default {@link WRITE_RESERVE}). */
  writeReserve?: number;
  /** EVENTs per minute window (default {@link LOCAL_EVENTS_PER_MINUTE}). */
  eventsPerMinute?: number;
  /** Minute window in ms (default {@link EVENT_MINUTE_WINDOW_MS}). */
  minuteWindowMs?: number;
  /** Clock (default `Date.now`). */
  now?: () => number;
  /** Timer (default `window.setTimeout`). */
  setTimeoutFn?: (fn: () => void, ms: number) => number;
  /** Timer cancel (default `window.clearTimeout`). */
  clearTimeoutFn?: (id: number) => void;
};

type Waiter = {
  lane: Exclude<SendLane, "free">;
  resolve: () => void;
};

/** Sliding-window token bucket with lane reserves. See the module doc. */
export class RelaySendBudget {
  private readonly capacity: number;
  private readonly windowMs: number;
  private readonly writeReserve: number;
  private readonly eventsPerMinute: number;
  private readonly minuteWindowMs: number;
  private readonly now: () => number;
  private readonly setTimeoutFn: (fn: () => void, ms: number) => number;
  private readonly clearTimeoutFn: (id: number) => void;

  /** Admission timestamps (ms) of every charged frame, oldest first. */
  private frames: number[] = [];
  /** Admission timestamps of every EVENT (durable or ephemeral). */
  private events: number[] = [];
  private waiters: Waiter[] = [];
  private wakeTimer: number | null = null;

  constructor(options: SendBudgetOptions = {}) {
    this.capacity = options.capacity ?? LOCAL_BURST_CAPACITY;
    this.windowMs = options.windowMs ?? BUDGET_WINDOW_MS;
    this.writeReserve = options.writeReserve ?? WRITE_RESERVE;
    this.eventsPerMinute = options.eventsPerMinute ?? LOCAL_EVENTS_PER_MINUTE;
    this.minuteWindowMs = options.minuteWindowMs ?? EVENT_MINUTE_WINDOW_MS;
    this.now = options.now ?? (() => Date.now());
    this.setTimeoutFn =
      options.setTimeoutFn ??
      ((fn, ms) => window.setTimeout(fn, ms) as unknown as number);
    this.clearTimeoutFn =
      options.clearTimeoutFn ?? ((id) => window.clearTimeout(id));
  }

  /** Frames charged inside the current burst window. */
  framesInWindow(): number {
    this.prune(this.now());
    return this.frames.length;
  }

  /** EVENTs charged inside the current minute window. */
  eventsInMinute(): number {
    this.prune(this.now());
    return this.events.length;
  }

  /**
   * Burst frames a lane could still take right now. Reads and ephemerals see
   * the capacity minus the write reserve; writes see all of it.
   */
  available(lane: Exclude<SendLane, "free">): number {
    this.prune(this.now());
    const reserve = lane === "write" ? 0 : this.writeReserve;
    const burst = this.capacity - reserve - this.frames.length;
    if (lane === "read") return Math.max(0, burst);
    const minute = this.eventsPerMinute - reserve - this.events.length;
    return Math.max(0, Math.min(burst, minute));
  }

  /**
   * Charge one frame to `lane` if a slot is free right now. Returns `false`
   * without charging when the lane is exhausted — the caller drops the frame.
   */
  tryAcquire(lane: SendLane): boolean {
    if (lane === "free") return true;
    if (this.available(lane) <= 0) return false;
    this.charge(lane, this.now());
    return true;
  }

  /**
   * Charge one frame to `lane`, waiting for a slot when none is free. Waiters
   * are woken in arrival order per lane; a write is admitted ahead of a read
   * waiting on the reserve.
   */
  acquire(lane: SendLane): Promise<void> {
    if (this.tryAcquire(lane)) return Promise.resolve();
    return new Promise<void>((resolve) => {
      this.waiters.push({
        lane: lane as Exclude<SendLane, "free">,
        resolve,
      });
      this.scheduleWake();
    });
  }

  /**
   * Drop every charge and release every waiter. Waiters resume without a slot
   * — the session-generation checks after their `await` are what stop a
   * superseded send, so releasing here is safe and prevents leaks across a
   * community switch.
   */
  reset(): void {
    this.frames = [];
    this.events = [];
    if (this.wakeTimer !== null) {
      this.clearTimeoutFn(this.wakeTimer);
      this.wakeTimer = null;
    }
    const waiters = this.waiters;
    this.waiters = [];
    for (const waiter of waiters) waiter.resolve();
  }

  private charge(lane: SendLane, at: number) {
    this.frames.push(at);
    if (lane === "write" || lane === "ephemeral") this.events.push(at);
  }

  private prune(at: number) {
    const burstFloor = at - this.windowMs;
    let drop = 0;
    while (drop < this.frames.length && this.frames[drop] <= burstFloor) drop++;
    if (drop > 0) this.frames = this.frames.slice(drop);
    const minuteFloor = at - this.minuteWindowMs;
    drop = 0;
    while (drop < this.events.length && this.events[drop] <= minuteFloor)
      drop++;
    if (drop > 0) this.events = this.events.slice(drop);
  }

  /** Admit as many waiters as slots allow, then arm a timer for the rest. */
  private drain() {
    this.wakeTimer = null;
    const remaining: Waiter[] = [];
    // Writes first: they own the reserve the other lanes are waiting behind.
    const ordered = [
      ...this.waiters.filter((w) => w.lane === "write"),
      ...this.waiters.filter((w) => w.lane !== "write"),
    ];
    for (const waiter of ordered) {
      if (this.tryAcquire(waiter.lane)) {
        waiter.resolve();
      } else {
        remaining.push(waiter);
      }
    }
    this.waiters = remaining;
    this.scheduleWake();
  }

  private scheduleWake() {
    if (this.waiters.length === 0 || this.wakeTimer !== null) return;
    const at = this.now();
    // The next slot opens when the oldest charged frame leaves its window.
    // Both windows are candidates; the earliest strictly-future expiry wins.
    const candidates: number[] = [];
    const oldestFrame = this.frames[0];
    if (oldestFrame !== undefined) {
      candidates.push(oldestFrame + this.windowMs - at + 1);
    }
    const oldestEvent = this.events[0];
    if (oldestEvent !== undefined) {
      candidates.push(oldestEvent + this.minuteWindowMs - at + 1);
    }
    const delay = Math.max(
      1,
      candidates.length > 0 ? Math.min(...candidates) : this.windowMs,
    );
    this.wakeTimer = this.setTimeoutFn(() => this.drain(), delay);
  }
}

let singleton = new RelaySendBudget();

/**
 * Charge the shared budget for one outbound frame. Returns `undefined` when a
 * slot was free (no await needed — the fast path stays synchronous), else a
 * promise that resolves once a slot is taken and `stillCurrent()` confirms
 * the session the frame was meant for is the one still connected, or
 * rejects with `supersededMessage`.
 */
export function admitFrame(
  payload: readonly unknown[],
  stillCurrent: () => boolean,
  supersededMessage: string,
): Promise<void> | undefined {
  const lane = classifySendLane(payload);
  if (singleton.tryAcquire(lane)) return undefined;
  return singleton.acquire(lane).then(() => {
    if (!stillCurrent()) throw new Error(supersededMessage);
  });
}

/** The process-wide budget every relay session send goes through. */
export function relaySendBudget(): RelaySendBudget {
  return singleton;
}

/**
 * Community-switch reset: the new relay meters a fresh key/community pair, so
 * charges against the old one must not pace sends to the new one.
 */
export function resetRelaySendBudget(): void {
  singleton.reset();
  singleton = new RelaySendBudget();
}
