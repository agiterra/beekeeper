import type { TrustedCodingSessionTranscriptEntry } from "./codingSessionTranscriptPresentation";
import type {
  TrustedCodingSessionIngressSnapshot,
  TrustedCodingSessionMetadataEntry,
} from "./codingSessionTrustedIngress";

/**
 * How a coalesced publish waits for the next frame. Injectable so a test can
 * step frames by hand; production reads the platform's own.
 */
export type CodingSessionIngressFrameScheduler = {
  request(callback: () => void): unknown;
  cancel(handle: unknown): void;
};

/**
 * A hidden or occluded webview may stop running animation frames altogether.
 * The ingress must not stall there — a create flow in a background window is
 * still waiting on its receipt — so every frame request carries a timer that
 * publishes anyway if no frame arrives first.
 */
const HIDDEN_WINDOW_FALLBACK_MS = 250;

/**
 * The platform's frame scheduler, or `null` where there is none (node tests,
 * a non-DOM host). `null` means "publish synchronously", which is exactly the
 * behaviour before coalescing existed — so an environment without frames is
 * never left holding an unpublished snapshot.
 *
 * Read at call time, not import time: a test may install `requestAnimationFrame`
 * after this module loaded.
 */
export function platformCodingSessionIngressFrameScheduler(): CodingSessionIngressFrameScheduler | null {
  const raf = globalThis.requestAnimationFrame;
  const caf = globalThis.cancelAnimationFrame;
  if (typeof raf !== "function" || typeof caf !== "function") return null;
  type Handle = { frame: number; timer: ReturnType<typeof setTimeout> };
  return {
    request(callback) {
      let done = false;
      const handle: Handle = { frame: 0, timer: 0 as never };
      const run = () => {
        if (done) return;
        done = true;
        caf(handle.frame);
        clearTimeout(handle.timer);
        callback();
      };
      handle.frame = raf(run);
      handle.timer = setTimeout(run, HIDDEN_WINDOW_FALLBACK_MS);
      return handle;
    },
    cancel(handle) {
      const { frame, timer } = handle as Handle;
      caf(frame);
      clearTimeout(timer);
    },
  };
}

export type CodingSessionIngressPublishCoalescer = {
  /** Publish on the next frame; any number of calls before it share one. */
  schedule(): void;
  /** Publish now, absorbing any frame already requested. */
  flushNow(): void;
  /** Drop a requested frame without publishing (teardown). */
  cancel(): void;
};

/**
 * Coalesce a burst of relay events into at most one publish per animation
 * frame.
 *
 * Every publish costs a full store snapshot and, downstream, a recomposed
 * catalog and every workspace derivation; a provider streaming a turn can
 * deliver dozens of events inside one frame, and only the last state of that
 * frame is ever painted. Control-flow publishes (loading, errors, the live
 * fence arming) call `flushNow` instead: they are rare, and their consumers
 * read them as immediate facts.
 */
export function createCodingSessionIngressPublishCoalescer(
  publish: () => void,
  scheduler: () => CodingSessionIngressFrameScheduler | null = platformCodingSessionIngressFrameScheduler,
): CodingSessionIngressPublishCoalescer {
  let pending: {
    scheduler: CodingSessionIngressFrameScheduler;
    handle: unknown;
  } | null = null;
  const cancel = () => {
    if (pending === null) return;
    pending.scheduler.cancel(pending.handle);
    pending = null;
  };
  return {
    schedule() {
      if (pending !== null) return;
      const frames = scheduler();
      if (frames === null) {
        publish();
        return;
      }
      let ran = false;
      const handle = frames.request(() => {
        ran = true;
        pending = null;
        publish();
      });
      // A scheduler that ran the callback synchronously has nothing pending.
      if (!ran) pending = { scheduler: frames, handle };
    },
    flushNow() {
      cancel();
      publish();
    },
    cancel,
  };
}

/**
 * Keep the previous snapshot's arrays — and each unchanged entry in them —
 * when the store's fresh snapshot says the same thing.
 *
 * `store.snapshot()` allocates new arrays and new entry objects every call, so
 * a transcript append also handed every consumer a "new" metadata list, and a
 * status change a "new" transcript list. Entries are matched by `eventId`, not
 * by position: the store sorts transcripts by channel, target, signer and
 * sequence, so one event inserted mid-array (any generation but the last, or
 * a late arrival) would otherwise give every later entry a new identity. A
 * matched prior entry is reused only when every field the store derives it
 * from agrees — an id match alone proves nothing — and the parsed payload is
 * the store's own retained object, so reference equality on it is exact. Each
 * prior entry is reused at most once, so duplicate ids cannot alias.
 *
 * The result always has the NEXT snapshot's order. Per array: the previous
 * array itself when the result is element-for-element identical to it; the
 * fresh array when no entry could be reused; otherwise a new array of reused
 * and fresh entries. When neither array differs from `next`'s own, `next` is
 * returned as is; otherwise a copy of it with the merged arrays.
 */
export function reuseCodingSessionIngressSnapshotArrays(
  previous: TrustedCodingSessionIngressSnapshot | null,
  next: TrustedCodingSessionIngressSnapshot,
): TrustedCodingSessionIngressSnapshot {
  if (previous === null) return next;
  const metadata = reuseEntries(
    previous.metadata,
    next.metadata,
    sameMetadataEntry,
  );
  const transcripts = reuseEntries(
    previous.transcripts,
    next.transcripts,
    sameTranscriptEntry,
  );
  if (metadata === next.metadata && transcripts === next.transcripts) {
    return next;
  }
  return { ...next, metadata, transcripts };
}

function reuseEntries<T extends { eventId: string }>(
  previous: T[],
  next: T[],
  same: (left: T, right: T) => boolean,
): T[] {
  if (previous.length === 0) return next.length === 0 ? previous : next;
  // eventId -> prior entries with that id, in prior order; a reused entry is
  // removed so it can be handed out only once.
  const priorById = new Map<string, T[]>();
  for (const entry of previous) {
    const bucket = priorById.get(entry.eventId);
    if (bucket === undefined) priorById.set(entry.eventId, [entry]);
    else bucket.push(entry);
  }
  let identical = previous.length === next.length;
  let reused = 0;
  const merged = next.map((entry, index) => {
    let result = entry;
    const bucket = priorById.get(entry.eventId);
    if (bucket !== undefined) {
      const at = bucket.findIndex((prior) => same(prior, entry));
      if (at !== -1) {
        result = bucket[at] as T;
        bucket.splice(at, 1);
        reused += 1;
      }
    }
    if (identical && result !== previous[index]) identical = false;
    return result;
  });
  if (identical) return previous;
  return reused === 0 ? next : merged;
}

function sameMetadataEntry(
  left: TrustedCodingSessionMetadataEntry,
  right: TrustedCodingSessionMetadataEntry,
): boolean {
  return (
    left.eventId === right.eventId &&
    left.channelId === right.channelId &&
    left.targetKey === right.targetKey &&
    left.signerPubkey === right.signerPubkey &&
    left.createdAt === right.createdAt &&
    left.conflictCount === right.conflictCount &&
    left.metadata === right.metadata
  );
}

function sameTranscriptEntry(
  left: TrustedCodingSessionTranscriptEntry,
  right: TrustedCodingSessionTranscriptEntry,
): boolean {
  return (
    left.eventId === right.eventId &&
    left.channelId === right.channelId &&
    left.targetKey === right.targetKey &&
    left.signerPubkey === right.signerPubkey &&
    left.createdAt === right.createdAt &&
    left.conflictCount === right.conflictCount &&
    left.transcript === right.transcript
  );
}
