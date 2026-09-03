/**
 * Pulse's one disk row, composed from what the host actually recorded.
 *
 * The row claims nothing it cannot back up: every worktree counted is one the
 * host recorded cutting, every gigabyte is measured, and every held entry
 * names its session and its file count so "3 held" is never a number with
 * nothing behind it.
 *
 * The unrecorded trees — the 65 that predate the record — are counted
 * separately and never folded into the removable total. A number that mixed
 * "the host will clean this up" with "somebody has to look at this" would be
 * worse than no number.
 */
import type { SeatWorktreeRow } from "@/shared/api/tauriCodingSessionWorktrees";

/** One held worktree, named. */
export type PulseDiskHeld = {
  key: string;
  sessionRef: string;
  seatLabel: string;
  path: string;
  dirtyFiles: number;
  /** `builder-1 · 3 uncommitted files`. */
  line: string;
};

/** The whole row, already worded. */
export type PulseDiskRow = {
  /** `disk: 12 worktrees, 184.0 GB reclaimable, 2 held`. */
  line: string;
  worktrees: number;
  /** Measured bytes, or `null` when any measurable row came back unknown. */
  reclaimableBytes: number | null;
  reclaimableLabel: string;
  held: PulseDiskHeld[];
  /** Trees the host found but never recorded. Listed, never adopted. */
  unrecorded: number;
  /** The sentence shown when nothing has been recorded at all. */
  empty: boolean;
};

const GIGABYTE = 1_000_000_000;

/** `{N} GB`, one decimal, or `unknown`. Mirrors the Rust renderer exactly. */
export function formatDiskBytes(bytes: number | null): string {
  if (bytes === null || !Number.isFinite(bytes)) return "unknown";
  return `${(bytes / GIGABYTE).toFixed(1)} GB`;
}

function pluralFiles(count: number): string {
  return count === 1 ? "1 uncommitted file" : `${count} uncommitted files`;
}

function pluralWorktrees(count: number): string {
  return count === 1 ? "1 worktree" : `${count} worktrees`;
}

/**
 * Compose the row from the host's classified rows.
 *
 * The reclaimable total counts only rows the host says may be reclaimed right
 * now (`reclaimableNow`), because a number that included directories nothing
 * will touch would be an invitation to expect space that never arrives.
 */
export function buildPulseDiskRow(
  rows: readonly SeatWorktreeRow[],
): PulseDiskRow {
  const unrecorded = rows.filter(
    (row) => row.disposition === "unrecorded",
  ).length;
  const held: PulseDiskHeld[] = rows
    .filter((row) => row.disposition === "held")
    .map((row) => ({
      key: row.key,
      sessionRef: row.sessionRef,
      seatLabel: row.seatLabel,
      path: row.path,
      dirtyFiles: row.dirtyFiles,
      line: `${row.seatLabel} · ${pluralFiles(row.dirtyFiles)}`,
    }));

  const reclaimable = rows.filter((row) => row.reclaimableNow);
  const anyUnknown = reclaimable.some((row) => row.reclaimableBytes === null);
  const bytes = anyUnknown
    ? null
    : reclaimable.reduce(
        (total, row) => total + (row.reclaimableBytes ?? 0),
        0,
      );
  const reclaimableLabel = formatDiskBytes(bytes);

  if (rows.length === 0) {
    return {
      line: "disk: no worktrees recorded on this machine",
      worktrees: 0,
      reclaimableBytes: 0,
      reclaimableLabel: formatDiskBytes(0),
      held: [],
      unrecorded: 0,
      empty: true,
    };
  }

  const parts = [
    pluralWorktrees(rows.length),
    `${reclaimableLabel} reclaimable`,
    `${held.length} held`,
  ];
  if (unrecorded > 0) {
    parts.push(`${unrecorded} unrecorded`);
  }

  return {
    line: `disk: ${parts.join(", ")}`,
    worktrees: rows.length,
    reclaimableBytes: bytes,
    reclaimableLabel,
    held,
    unrecorded,
    empty: false,
  };
}
