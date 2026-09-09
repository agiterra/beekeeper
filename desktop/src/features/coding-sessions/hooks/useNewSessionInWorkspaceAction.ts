import * as React from "react";
import { toast } from "sonner";

import {
  useCodingSessionWorkspaceReuse,
  type CodingSessionWorkspaceReuseDeps,
} from "@/features/coding-sessions/hooks/useCodingSessionWorkspaceReuse";
import type { WorkspaceReuseResolution } from "@/features/coding-sessions/lib/codingSessionWorkspaceReuse";
import {
  newSessionInWorkspaceMenuDetail,
  WORKSPACE_REUSE_CHOOSE_FOLDER_SENTENCE,
} from "@/features/coding-sessions/lib/codingSessionWorkspaceReuseCopy";
import {
  openNewCodingSessionDialog,
  openNewCodingSessionDialogInWorkspace,
} from "@/features/coding-sessions/newCodingSessionDialogStore";

/**
 * "New session in this workspace", as one action both entry points call.
 *
 * The sidebar row's context menu and the open session's header overflow ask
 * for the same thing, and one of them would eventually answer differently if
 * each wired its own read. So the resolution, the choice of which draft to
 * open, and the sentence shown when there is nothing to reuse all live here,
 * over Lane W's single `useCodingSessionWorkspaceReuse`.
 *
 * What a click does, exhaustively: it reads (a worktree list, a directory
 * validation, a provider comparison — all reads), and it opens a draft. It
 * signs nothing, starts, resumes or stops nothing, changes no branch and no
 * grant, and creates no directory. The launcher the draft opens is the one
 * "New session" has always opened; a reuse draft simply arrives with the
 * folder already filled in and worktree creation off.
 *
 * When the workspace cannot be honoured here — no recorded directory, a
 * directory that is gone, an execution on a provider this computer does not
 * hold, or a read that failed outright — the ordinary launcher opens with
 * **nothing seeded** and the reason is said out loud. It never substitutes a
 * different directory, and it never implies this computer could reach another
 * machine's checkout.
 */
export function useNewSessionInWorkspaceAction(input: {
  channelId: string | null;
  /** Null for a row with no durable session ref: nothing to look up. */
  sessionRef: string | null;
  projectId?: string | null;
  /**
   * The viewed execution's provider authority, when the caller holds it.
   *
   * Without it the comparison cannot run and every answer degrades to "where
   * its execution runs is not known here" — never to `elsewhere`, which is
   * why omitting it is safe but lossy: the foreign case cannot render at all.
   */
  executionProviderPubkey?: string | null;
  /** Injected only by tests. */
  deps?: CodingSessionWorkspaceReuseDeps;
}): {
  /** The menu's second line — unresolved wording until a read answers. */
  detail: string;
  /** Call when the menu opens. Reads on open, never on render or hover. */
  resolveNow: () => void;
  /** The click. Opens a draft; nothing else. */
  start: () => void;
} {
  const { channelId, executionProviderPubkey = null, sessionRef } = input;
  const projectId = input.projectId ?? null;
  // Hooks are unconditional, so the read hook is always called; a session
  // with no ref is simply never asked to read (`hasSession` below).
  const reuse = useCodingSessionWorkspaceReuse(sessionRef ?? "", channelId, {
    deps: input.deps,
    executionProviderPubkey,
  });
  const hasSession = sessionRef !== null && sessionRef.length > 0;
  // A second click while the first read is in flight would open two drafts.
  const busy = React.useRef(false);
  const { resolve } = reuse;

  const resolveNow = React.useCallback(() => {
    if (!hasSession) return;
    void resolve().catch(() => {
      // The hook reports a failed read through `error`; a rejection here has
      // nothing left to show and must not take the menu down with it.
    });
  }, [hasSession, resolve]);

  const start = React.useCallback(() => {
    if (busy.current) return;
    busy.current = true;
    const open = (workspace: WorkspaceReuseResolution | null) => {
      if (workspace !== null && workspace.path !== null && hasSession) {
        openNewCodingSessionDialogInWorkspace({
          channelId,
          projectId,
          sessionRef: sessionRef ?? "",
          workspace: {
            path: workspace.path,
            branch: workspace.branch,
            // Only the creation-time fact travels. A live head read now would
            // be restored from `sessionStorage` after a reload and shown as
            // current long after it stopped being — so the draft re-reads
            // that itself, and anything other than `recorded` is dropped
            // here rather than relabelled.
            branchSource:
              workspace.branchSource === "recorded" ? "recorded" : null,
          },
        });
        return;
      }
      // Nothing seeded. The ordinary launcher, exactly as "New session"
      // opens it, with the folder picker the person can drive themselves.
      openNewCodingSessionDialog(channelId);
    };
    const finish = () => {
      busy.current = false;
    };
    if (!hasSession) {
      open(null);
      finish();
      return;
    }
    void resolve()
      .then((resolution) => {
        if (resolution.availability === "available" && resolution.path) {
          open(resolution);
          return;
        }
        open(null);
        // Said in the same breath as the draft opening, rather than left to
        // be inferred from an empty field.
        toast.info(resolution.sentence, {
          description: WORKSPACE_REUSE_CHOOSE_FOLDER_SENTENCE,
        });
      })
      .catch(() => {
        // A read that threw is not an available workspace, and it is not
        // evidence of an absence either — so the draft opens unseeded and
        // says only what happened.
        open(null);
        toast.info(
          "This computer could not read this session's directories just now.",
          { description: WORKSPACE_REUSE_CHOOSE_FOLDER_SENTENCE },
        );
      })
      .finally(finish);
  }, [channelId, hasSession, projectId, resolve, sessionRef]);

  return {
    detail: newSessionInWorkspaceMenuDetail(reuse.resolution, reuse.error),
    resolveNow,
    start,
  };
}
