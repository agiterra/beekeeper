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
  requestCodingSessionFounding,
  requestCodingSessionFoundingInWorkspace,
} from "@/features/coding-sessions/newCodingSessionDialogStore";

/**
 * "New session in this workspace", as one action both entry points call.
 *
 * The sidebar row's context menu and the open session's header overflow ask
 * for the same thing, and one of them would eventually answer differently if
 * each wired its own read. So the resolution, the choice of which request to
 * write, and the sentence shown when there is nothing to reuse all live here,
 * over Lane W's single `useCodingSessionWorkspaceReuse`.
 *
 * What a click does, exhaustively: it reads (a worktree list, a directory
 * validation, a provider comparison — all reads), and it writes a founding
 * request. The host in the app shell then founds the session — one genesis —
 * and opens its page, where the folder arrives already filled in and worktree
 * creation off. This action itself signs nothing, starts, resumes or stops
 * nothing, changes no branch and no grant, and creates no directory.
 *
 * When the workspace cannot be honoured here — no recorded directory, a
 * directory that is gone, an execution on a provider this computer does not
 * hold, or a read that failed outright — the ordinary request is written
 * with **nothing seeded** and the reason is said out loud. It never
 * substitutes a different directory, and it never implies this computer could
 * reach another machine's checkout.
 */
export function useNewSessionInWorkspaceAction(input: {
  channelId: string | null;
  /** Null for a row with no durable session ref: nothing to look up. */
  sessionRef: string | null;
  projectId?: string | null;
  /** Repository coordinate from the viewed execution's verified metadata. */
  sourceRepoRef?: string | null;
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
  /** The click. Writes a founding request; nothing else. */
  start: () => void;
} {
  const { channelId, executionProviderPubkey = null, sessionRef } = input;
  const projectId = input.projectId ?? null;
  const sourceRepoRef = input.sourceRepoRef ?? null;
  // Hooks are unconditional, so the read hook is always called; a session
  // with no ref is simply never asked to read (`hasSession` below).
  const reuse = useCodingSessionWorkspaceReuse(sessionRef ?? "", channelId, {
    deps: input.deps,
    executionProviderPubkey,
  });
  const hasSession = sessionRef !== null && sessionRef.length > 0;
  // A second click while the first read is in flight would write a second
  // request once the first cleared — the store refuses one while a request
  // stands, but not after it has been founded and cleared.
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
        requestCodingSessionFoundingInWorkspace({
          channelId,
          projectId,
          sourceRepoRef,
          sessionRef: sessionRef ?? "",
          workspace: {
            path: workspace.path,
            branch: workspace.branch,
            // Only the creation-time fact travels. A live head read now would
            // be carried into the founded draft and shown as current long
            // after it stopped being — so the page re-reads that itself, and
            // anything other than `recorded` is dropped here rather than
            // relabelled.
            branchSource:
              workspace.branchSource === "recorded" ? "recorded" : null,
          },
        });
        return;
      }
      // Nothing seeded. The ordinary founding, exactly as "New session"
      // requests it; the page's folder picker is the person's to drive.
      requestCodingSessionFounding(channelId);
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
        // Said in the same breath as the founding, rather than left to be
        // inferred from an empty field.
        toast.info(resolution.sentence, {
          description: WORKSPACE_REUSE_CHOOSE_FOLDER_SENTENCE,
        });
      })
      .catch(() => {
        // A read that threw is not an available workspace, and it is not
        // evidence of an absence either — so the request goes unseeded and
        // says only what happened.
        open(null);
        toast.info(
          "This computer could not read this session's directories just now.",
          { description: WORKSPACE_REUSE_CHOOSE_FOLDER_SENTENCE },
        );
      })
      .finally(finish);
  }, [channelId, hasSession, projectId, resolve, sessionRef, sourceRepoRef]);

  return {
    detail: newSessionInWorkspaceMenuDetail(reuse.resolution, reuse.error),
    resolveNow,
    start,
  };
}
