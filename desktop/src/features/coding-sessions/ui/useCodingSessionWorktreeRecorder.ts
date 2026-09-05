import * as React from "react";

import {
  type CodingSessionLifecycleResolution,
  establishedCodingSessionTarget,
} from "@/features/coding-sessions/lib/codingSessionTrustedIngress";
import { recordCodingSessionWorktree } from "@/shared/api/tauriCodingSessionWorktrees";

/**
 * The second half of a worktree create.
 *
 * A worktree is cut before the genesis is signed, so at that moment there is
 * no session to record it against — and a tree the host never recorded is one
 * it can never name and will never remove, which is how they accumulate. The
 * directory therefore has to survive from submit until the session settles,
 * which is the first moment it has a name.
 *
 * It is held in memory rather than in the transaction record on purpose: a
 * working directory is host-local and must never travel in a relay event. A
 * reload in that window loses it and the tree goes unrecorded, exactly as it
 * did before this existed — no worse, and not worth persisting a path for.
 *
 * Extracted from `useNewCodingSessionCreate` because that file sits at the
 * 1000-line ceiling; this is one concern with one seam, so it is the natural
 * piece to lift out.
 */
export function useCodingSessionWorktreeRecorder() {
  const pendingRef = React.useRef(new Map<string, string>());

  return React.useMemo(
    () => ({
      /** Hold the directory this create will run in, until it settles. */
      remember(commandId: string, workdir: string | null) {
        if (workdir) pendingRef.current.set(commandId, workdir);
      },
      /**
       * Record the directory against the session that just settled.
       *
       * The lifecycle resolution is passed rather than a session id because
       * the id is on the established target and this is the one place that
       * needs it. It lets the host tell the provider where this session's
       * tree is *now*, at every gate, instead of only where it was at create
       * (finding 82). A lifecycle that establishes no target records the
       * directory with no session id, which is the honest shape: the tree is
       * still named, and no session claims it.
       *
       * Safe to call for every settled session: the host resolves the
       * directory to its repository and records only what it recognises as a
       * worktree it cut, so an ordinary checkout records nothing. Failure is
       * swallowed — the session is already established, and not remembering
       * its directory must not read as the launch having failed.
       */
      settle(
        commandId: string,
        sessionRef: string,
        lifecycle?: CodingSessionLifecycleResolution | null,
      ) {
        const workdir = pendingRef.current.get(commandId);
        pendingRef.current.delete(commandId);
        if (!workdir) return;
        void recordCodingSessionWorktree({
          sessionRef,
          seatLabel: "lead",
          path: workdir,
          // The execution's own session id, which the host republishes to the
          // provider so a gate row is measured in the tree this session is
          // actually in — not the path its create resolved (finding 82).
          sessionId:
            establishedCodingSessionTarget(lifecycle)?.sessionId ?? null,
        }).catch(() => {});
      },
    }),
    [],
  );
}
