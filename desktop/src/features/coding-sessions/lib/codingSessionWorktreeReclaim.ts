/**
 * What the close dialog says about a session's worktrees, and what it offers.
 *
 * Every disposition was decided in Rust. This module only decides *layout*:
 * which of the host's sentences appear before the confirm, which of them get a
 * control beside them, and how the reclaimable total is worded.
 *
 * Two rules it exists to enforce:
 *
 * * **A session closes whether or not its worktrees can go.** Closing is a
 *   published fact about the session; removing a directory is a separate act
 *   with a separate confirm. Nothing here can make the close conditional on a
 *   directory, and nothing here removes anything.
 * * **Unknown is not zero.** A directory whose size could not be measured
 *   reads `unknown`, and a total that includes one is not claimed as a total.
 */
import type {
  SeatWorktreeDisposition,
  SeatWorktreeRow,
} from "@/shared/api/tauriCodingSessionWorktrees";

/** One line the dialog shows, and the control (if any) that belongs to it. */
export type WorktreeClosureLine = {
  key: string;
  sessionRef: string;
  seatLabel: string;
  path: string;
  /** The host's own sentence, rendered verbatim. */
  detail: string;
  disposition: SeatWorktreeDisposition;
  dirtyFiles: number;
  /** A **Prune held worktree** button belongs on this row. */
  offersPrune: boolean;
  /** A **Reclaim build output** control belongs on this row. */
  offersReclaim: boolean;
  reclaimableLabel: string;
};

/** Everything the dialog needs, composed once. */
export type WorktreeClosureSummary = {
  lines: WorktreeClosureLine[];
  /** Rows holding uncommitted work; the session still closes. */
  heldCount: number;
  /** Total uncommitted files across every held row. */
  heldFiles: number;
  /** `{N} GB reclaimable`, or `unknown` when any row could not be measured. */
  reclaimableLabel: string;
  /** Sum of the measurable rows, or `null` when any row was unmeasurable. */
  reclaimableBytes: number | null;
  /** True when at least one row can have its build output removed now. */
  offersReclaim: boolean;
};

/**
 * Bytes as `{N} GB`, one decimal — the same shape
 * `beekeeper_core::worktree_lifecycle::render_reclaimable_bytes` prints, so the CLI
 * and the app never word the same number two ways.
 */
export function formatReclaimableBytes(bytes: number | null): string {
  if (bytes === null || !Number.isFinite(bytes)) return "unknown";
  return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
}

/**
 * A **Prune held worktree** button appears on a `held` row and nowhere else.
 *
 * Not on `prunable` (the host removes that one itself, and the dialog already
 * says so), and never on `protected`, `unrecorded`, `execution-live` or
 * `not-settled`, where the answer is a reason rather than an offer.
 */
export function offersPruneControl(row: SeatWorktreeRow): boolean {
  return row.disposition === "held" && row.exists;
}

/** Build output is offered wherever the host says it may go right now. */
export function offersReclaimControl(row: SeatWorktreeRow): boolean {
  return row.reclaimableNow && row.exists && row.reclaimableBytes !== 0;
}

/** Compose the closure dialog's worktree section from the host's rows. */
export function summarizeWorktreeClosure(
  rows: readonly SeatWorktreeRow[],
): WorktreeClosureSummary {
  const lines: WorktreeClosureLine[] = rows.map((row) => ({
    key: row.key,
    sessionRef: row.sessionRef,
    seatLabel: row.seatLabel,
    path: row.path,
    detail: row.detail,
    disposition: row.disposition,
    dirtyFiles: row.dirtyFiles,
    offersPrune: offersPruneControl(row),
    offersReclaim: offersReclaimControl(row),
    reclaimableLabel: row.reclaimableLabel,
  }));

  const held = rows.filter((row) => row.disposition === "held");
  const reclaimable = rows.filter((row) => row.reclaimableNow);
  // One unmeasurable directory makes the *total* unknown. A partial sum
  // presented as a whole one is a number nobody can act on.
  const anyUnknown = reclaimable.some((row) => row.reclaimableBytes === null);
  const bytes = anyUnknown
    ? null
    : reclaimable.reduce(
        (total, row) => total + (row.reclaimableBytes ?? 0),
        0,
      );

  return {
    lines,
    heldCount: held.length,
    heldFiles: held.reduce((total, row) => total + row.dirtyFiles, 0),
    reclaimableBytes: bytes,
    reclaimableLabel: formatReclaimableBytes(bytes),
    offersReclaim: lines.some((line) => line.offersReclaim),
  };
}

/**
 * The sentence a **Prune held worktree** confirm shows before removing one.
 *
 * It names the path and the count, because those are the two facts a person
 * needs to decide, and because a confirm that says only "are you sure" is a
 * confirm that teaches people to click through.
 */
export function pruneHeldConfirmCopy(line: WorktreeClosureLine): string {
  const files =
    line.dirtyFiles === 1
      ? "1 uncommitted file"
      : `${line.dirtyFiles} uncommitted files`;
  return `Remove ${line.path}? It holds ${files}, and they are not recoverable from this app afterwards.`;
}
