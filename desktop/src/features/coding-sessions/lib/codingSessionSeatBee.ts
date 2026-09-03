/**
 * Which `bee` a seat is actually running, as the host observed it.
 *
 * A seat runs whatever `bee` its harness put on `PATH`. On 2026-09-01 that was
 * the app-bundled sidecar for one seat and the hot checkout's debug build for
 * another — one run, two binaries answering about one channel, and neither
 * surface said so. The host now resolves one binary, runs `$BEE --version`
 * itself, and records the result on the seat's kind:44223 metadata as the
 * additive optional `beeStamp` key. This module is the reader for that key and
 * the frozen copy for the two surfaces that disclose it.
 *
 * Two rules the rest of the app depends on. First, absence is not unknown: a
 * 44223 published by an older host carries no `beeStamp` at all, and a surface
 * must render nothing rather than invent an "unknown" nobody published. A host
 * that ran `--version` and could not parse it *does* publish, with null
 * version/sha/dirty, and that reads **unknown**. Second, ancestry is decided by
 * the Desktop host against the local checkout — never here. TypeScript never
 * compares two sha strings to guess which build is older.
 */

/** Where the host found the binary: the bundled sidecar, or the first on `PATH`. */
export type SeatBeeSource = "bundled" | "path";

/**
 * One seat's observed `bee`, exactly as the wire carries it.
 *
 * All five keys are always present when the key exists; an optional value is
 * JSON `null`, never absent. `sha` is the short commit without any `-dirty`
 * suffix — `dirty` carries that separately, so the two facts stay separable.
 */
export type SeatBeeStamp = {
  /** Absolute path to the binary the host chose. */
  path: string;
  /** How the host found it. */
  source: SeatBeeSource;
  /** The version string `bee --version` printed, or `null` if it did not parse. */
  version: string | null;
  /** Short commit, lowercase hex 7–40 chars, or `null` if it did not parse. */
  sha: string | null;
  /** Whether the build's tree was dirty, or `null` if it did not parse. */
  dirty: boolean | null;
};

const SEAT_BEE_STAMP_KEYS = [
  "path",
  "source",
  "version",
  "sha",
  "dirty",
] as const;

const SHORT_SHA = /^[0-9a-f]{7,40}$/;

/**
 * Read a `beeStamp` from an untrusted wire value, or `null` for "no stamp".
 *
 * Strict on purpose: exactly the five known keys, each on its own terms. A
 * shape this reader does not fully recognise yields `null` — the surfaces then
 * render nothing — rather than a partially guessed stamp that would let a chip
 * claim a build the host never observed.
 */
export function readSeatBeeStamp(value: unknown): SeatBeeStamp | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return null;
  }
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record);
  if (keys.length !== SEAT_BEE_STAMP_KEYS.length) return null;
  for (const key of SEAT_BEE_STAMP_KEYS) {
    if (!Object.hasOwn(record, key)) return null;
  }
  const { path, source, version, sha, dirty } = record;
  if (typeof path !== "string" || path.length === 0) return null;
  if (source !== "bundled" && source !== "path") return null;
  if (version !== null && typeof version !== "string") return null;
  if (sha !== null && (typeof sha !== "string" || !SHORT_SHA.test(sha))) {
    return null;
  }
  if (dirty !== null && typeof dirty !== "boolean") return null;
  return { path, source, version, sha, dirty };
}

/**
 * The frozen seat-card line, or `null` when the wire carried no stamp.
 *
 * Words, never a colour: `bee 23728227b (bundled)`, `bee 07c470be0-dirty
 * (found on PATH: /Users/…/target/debug)`, or `bee build unknown` when the
 * host ran `--version` and could not parse it.
 */
export function codingSessionSeatBeeLine(
  stamp: SeatBeeStamp | null,
): string | null {
  if (stamp === null) return null;
  if (stamp.sha === null) return "bee build unknown";
  const build = `${stamp.sha}${stamp.dirty === true ? "-dirty" : ""}`;
  if (stamp.source === "bundled") return `bee ${build} (bundled)`;
  return `bee ${build} (found on PATH: ${seatBeeDirectory(stamp.path)})`;
}

/** The binary's own directory: its path with the last segment removed. */
function seatBeeDirectory(path: string): string {
  const cut = path.lastIndexOf("/");
  if (cut < 0) return path;
  return path.slice(0, cut);
}

/**
 * Where the host placed a seat's build relative to `main`.
 *
 * Computed in the Desktop host against the local checkout. `unknown` is a real
 * answer — the host could not decide — and is disclosed as such, never folded
 * into "up to date".
 */
export type SeatBeeAncestry =
  | { kind: "on-main" }
  | { kind: "behind"; commits: number }
  | { kind: "unknown" };

/** One live seat as Pulse sees it, with whatever stamp its 44223 carried. */
export type PulseStaleBeeSeat = {
  seatKey: string;
  label: string;
  stamp: SeatBeeStamp | null;
};

/** One seat Pulse can say is behind `main`, with the host's own number. */
export type PulseStaleBeeRow = {
  seatKey: string;
  label: string;
  sha7: string;
  behind: number;
};

/** What Pulse's "seats running an older bee" card renders, and nothing more. */
export type PulseStaleBeeReading = {
  rows: PulseStaleBeeRow[];
  /** Seats with no stamp, no sha, or no host ancestry answer. */
  uncomparedCount: number;
  /** Rows the {@link PULSE_STALE_BEE_ROW_LIMIT} cap dropped, for disclosure. */
  truncatedCount: number;
};

/** The most rows the card lists; the rest are disclosed as a count. */
export const PULSE_STALE_BEE_ROW_LIMIT = 20;

/**
 * Fold live seats and the host's ancestry answers into the Pulse card's rows.
 *
 * A seat only becomes a row when the host said, in commits, how far behind
 * `main` its build is. Anything else — no stamp, an unparsed `--version`, a
 * sha the host was not asked about, or an ancestry it could not decide —
 * counts toward `uncomparedCount` and is never listed as owed work. Order is
 * deterministic: furthest behind first, ties by label byte order.
 */
export function buildPulseStaleBeeReading(
  seats: readonly PulseStaleBeeSeat[],
  ancestryBySha: ReadonlyMap<string, SeatBeeAncestry>,
): PulseStaleBeeReading {
  const rows: PulseStaleBeeRow[] = [];
  let uncomparedCount = 0;
  for (const seat of seats) {
    const sha = seat.stamp?.sha ?? null;
    if (sha === null) {
      uncomparedCount += 1;
      continue;
    }
    const ancestry = ancestryBySha.get(sha);
    if (ancestry === undefined || ancestry.kind === "unknown") {
      uncomparedCount += 1;
      continue;
    }
    if (ancestry.kind === "on-main") continue;
    rows.push({
      seatKey: seat.seatKey,
      label: seat.label,
      sha7: sha.slice(0, 7),
      behind: ancestry.commits,
    });
  }
  rows.sort((left, right) => {
    if (left.behind !== right.behind) return right.behind - left.behind;
    if (left.label === right.label) return 0;
    return left.label < right.label ? -1 : 1;
  });
  const truncatedCount = Math.max(0, rows.length - PULSE_STALE_BEE_ROW_LIMIT);
  return {
    rows: rows.slice(0, PULSE_STALE_BEE_ROW_LIMIT),
    uncomparedCount,
    truncatedCount,
  };
}
