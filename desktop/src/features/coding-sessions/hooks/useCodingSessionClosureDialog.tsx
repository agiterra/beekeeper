import * as React from "react";
import { toast } from "sonner";

import {
  type CodingSessionClosureAction,
  codingSessionClosureIsClosed,
  publishCodingSessionClosure,
} from "@/features/coding-sessions/lib/codingSessionClosure";
import {
  type WorktreeClosureLine,
  type WorktreeClosureSummary,
  pruneHeldConfirmCopy,
  summarizeWorktreeClosure,
} from "@/features/coding-sessions/lib/codingSessionWorktreeReclaim";
import {
  closeCodingSessionSeatWorktree,
  listCodingSessionSeatWorktrees,
  pruneCodingSessionSeatWorktree,
  reclaimCodingSessionSeatWorktree,
} from "@/shared/api/tauriCodingSessionWorktrees";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/shared/ui/alert-dialog";
import { Button } from "@/shared/ui/button";

export type CodingSessionClosureRequest = {
  action: CodingSessionClosureAction;
  channelId: string;
  genesisRef: string;
  label: string;
  sessionRef: string;
};

const COPY: Record<
  CodingSessionClosureAction,
  {
    title: string;
    confirm: string;
    failed: string;
    describe: (label: string) => string;
  }
> = {
  closed: {
    title: "Close this session?",
    confirm: "Close session",
    failed: "Failed to close the session.",
    describe: (label) =>
      `"${label}" will move to Closed for every project member. ` +
      "Its identity and transcript remain available. Any execution still " +
      "running on its host is stopped and its slot freed.",
  },
  archived: {
    title: "Archive this session?",
    confirm: "Archive session",
    failed: "Failed to archive the session.",
    describe: (label) =>
      `"${label}" will be closed and archived for every project member: it ` +
      "leaves the sidebar unless a filter asks for archived sessions, but its " +
      "identity and transcript remain available and it can be reopened. " +
      "Any execution still running on its host is stopped and its slot " +
      "freed.",
  },
  open: {
    title: "Reopen this session?",
    confirm: "Reopen session",
    failed: "Failed to reopen the session.",
    describe: (label) =>
      `"${label}" will return to open sessions for every project member. ` +
      "Reopening starts no provider and consumes no execution slot.",
  },
};

/**
 * The worktrees a session's seats are still holding on this machine.
 *
 * Loaded once when the dialog opens — no polling, no interval. A host that
 * does not answer (an older build, or the mock bridge in a smoke test) leaves
 * the section absent rather than showing an empty one: a blank list under a
 * heading reads as "there are none", which would be a claim.
 */
function useSeatWorktrees(
  target: CodingSessionClosureRequest | null,
): WorktreeClosureSummary | null {
  const [summary, setSummary] = React.useState<WorktreeClosureSummary | null>(
    null,
  );
  const sessionRef = target?.sessionRef ?? null;
  const action = target?.action ?? null;

  React.useEffect(() => {
    if (!sessionRef || !action) {
      setSummary(null);
      return;
    }
    let cancelled = false;
    void listCodingSessionSeatWorktrees([
      {
        sessionRef,
        // What the confirm is about to make true. A close settles the
        // umbrella, which is exactly the fact the dispositions turn on.
        sessionSettled: codingSessionClosureIsClosed(action),
        // The host stops a settled session's executions; nothing here claims
        // to have observed one still running.
        executionLive: false,
        // This surface does not fetch kind 30618, so it has not established
        // where the branch stands on the relay. `null`, never `false`.
        tipOnRelay: null,
        // Settling now: the grace window runs from this moment.
        settledForSecs: codingSessionClosureIsClosed(action) ? 0 : null,
      },
    ])
      .then((rows) => {
        if (!cancelled) setSummary(summarizeWorktreeClosure(rows));
      })
      .catch(() => {
        if (!cancelled) setSummary(null);
      });
    return () => {
      cancelled = true;
    };
  }, [sessionRef, action]);

  return summary;
}

function WorktreeLine({
  line,
  onPrune,
  onReclaim,
  busy,
}: {
  line: WorktreeClosureLine;
  onPrune: (line: WorktreeClosureLine) => void;
  onReclaim: (line: WorktreeClosureLine) => void;
  busy: boolean;
}) {
  return (
    <li
      className="flex items-start justify-between gap-2"
      data-disposition={line.disposition}
      data-testid="coding-session-closure-worktree"
    >
      <span className="text-2xs text-muted-foreground" title={line.path}>
        {line.detail}
      </span>
      <span className="flex shrink-0 gap-1">
        {line.offersReclaim ? (
          <Button
            data-testid="coding-session-worktree-reclaim"
            disabled={busy}
            onClick={() => onReclaim(line)}
            size="sm"
            variant="outline"
          >
            Reclaim {line.reclaimableLabel}
          </Button>
        ) : null}
        {line.offersPrune ? (
          <Button
            data-testid="coding-session-worktree-prune"
            disabled={busy}
            onClick={() => onPrune(line)}
            size="sm"
            variant="outline"
          >
            Prune held worktree
          </Button>
        ) : null}
      </span>
    </li>
  );
}

/**
 * Shared confirmation for the close, archive and reopen facts.
 *
 * The copy used to end "Closing does not stop provider executions; stop
 * those separately when needed." That was true and is not any more: the
 * host now subscribes to closure revisions and stops the executions a
 * settled umbrella still holds, because before it did, a closed session went
 * on occupying one of the host's (default ten) session slots until a
 * four-hour idle timeout — and the only symptom was a later create refused
 * with a limit the operator could not reconcile with what they saw.
 *
 * The reopen copy still says a reopen starts no provider and consumes no
 * slot, which remains exactly true.
 *
 * **The worktree section, added by L11, never gates the close.** Closing is a
 * published fact about the session; a directory on one machine is a separate
 * act with its own confirm. A seat worktree holding uncommitted work is
 * disclosed as `held: {N} uncommitted files` and is **left on disk** — the
 * session closes and the directory stays, because a person's uncommitted work
 * is never deleted as a side effect of anything.
 */
export function useCodingSessionClosureDialog(): {
  requestClosure: (request: CodingSessionClosureRequest | null) => void;
  dialog: React.ReactNode;
} {
  const [target, setTarget] =
    React.useState<CodingSessionClosureRequest | null>(null);
  const [publishing, setPublishing] = React.useState(false);
  const [busyKey, setBusyKey] = React.useState<string | null>(null);
  const [held, setHeld] = React.useState<WorktreeClosureLine | null>(null);
  const summary = useSeatWorktrees(target);

  const requestClosure = React.useCallback(
    (request: CodingSessionClosureRequest | null) => {
      if (request) setTarget(request);
    },
    [],
  );

  const confirm = React.useCallback(() => {
    if (!target) return;
    const lines = summary?.lines ?? [];
    setPublishing(true);
    void publishCodingSessionClosure(target)
      .then(() => {
        // SESSION_STATE §3 item 9: the host disposes of its own seat trees on
        // a close instead of waiting for somebody to run `bee sessions
        // worktree prune`. It is told nothing about what may go — it decides
        // with the shared predicate and answers with a sentence either way, so
        // a tree holding uncommitted work is kept here exactly as it is kept
        // everywhere else. Best effort and never surfaced as an error: the
        // closure is already a published fact, and a directory this machine
        // could not tidy must not read as the close having failed.
        if (!codingSessionClosureIsClosed(target.action)) return;
        for (const line of lines) {
          void closeCodingSessionSeatWorktree({
            sessionRef: line.sessionRef,
            seatLabel: line.seatLabel,
            executionLive: false,
            settledForSecs: 0,
          }).catch(() => {});
        }
      })
      .catch((error: unknown) => {
        toast.error(
          error instanceof Error ? error.message : COPY[target.action].failed,
        );
      })
      .finally(() => {
        setPublishing(false);
        setTarget(null);
      });
  }, [summary, target]);

  const reclaim = React.useCallback((line: WorktreeClosureLine) => {
    setBusyKey(line.key);
    void reclaimCodingSessionSeatWorktree({
      sessionRef: line.sessionRef,
      seatLabel: line.seatLabel,
    })
      .then((result) => {
        toast.success(`Reclaimed ${result.freedLabel} from ${line.path}`);
      })
      .catch((error: unknown) => {
        toast.error(
          error instanceof Error
            ? error.message
            : "Failed to reclaim the build output.",
        );
      })
      .finally(() => setBusyKey(null));
  }, []);

  const confirmPrune = React.useCallback(() => {
    if (!held) return;
    const line = held;
    setBusyKey(line.key);
    setHeld(null);
    void pruneCodingSessionSeatWorktree({
      sessionRef: line.sessionRef,
      seatLabel: line.seatLabel,
    })
      .then((path) => toast.success(`Removed ${path}`))
      .catch((error: unknown) => {
        toast.error(
          error instanceof Error
            ? error.message
            : "Failed to remove the worktree.",
        );
      })
      .finally(() => setBusyKey(null));
  }, [held]);

  const copy = target ? COPY[target.action] : null;
  const dialog = (
    <>
      <AlertDialog
        open={target !== null}
        onOpenChange={(open) => {
          if (!open) setTarget(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{copy?.title ?? ""}</AlertDialogTitle>
            <AlertDialogDescription>
              {target && copy ? copy.describe(target.label) : ""}
            </AlertDialogDescription>
          </AlertDialogHeader>
          {summary && summary.lines.length > 0 ? (
            <div data-testid="coding-session-closure-worktrees">
              <p className="text-xs font-medium text-foreground">
                Worktrees on this machine
              </p>
              <ul className="mt-1 space-y-1">
                {summary.lines.map((line) => (
                  <WorktreeLine
                    busy={busyKey === line.key}
                    key={line.key}
                    line={line}
                    onPrune={setHeld}
                    onReclaim={reclaim}
                  />
                ))}
              </ul>
              {summary.heldCount > 0 ? (
                <p
                  className="mt-1 text-2xs text-muted-foreground"
                  data-testid="coding-session-closure-held-note"
                >
                  The session closes either way; nothing holding uncommitted
                  work is removed.
                </p>
              ) : null}
            </div>
          ) : null}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={publishing}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              data-testid="coding-session-closure-confirm"
              disabled={publishing}
              onClick={(event) => {
                event.preventDefault();
                confirm();
              }}
            >
              {copy?.confirm ?? ""}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
      <AlertDialog
        open={held !== null}
        onOpenChange={(open) => {
          if (!open) setHeld(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Remove this worktree?</AlertDialogTitle>
            <AlertDialogDescription>
              {held ? pruneHeldConfirmCopy(held) : ""}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              data-testid="coding-session-worktree-prune-confirm"
              onClick={(event) => {
                event.preventDefault();
                confirmPrune();
              }}
            >
              Remove worktree
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );

  return { requestClosure, dialog };
}
