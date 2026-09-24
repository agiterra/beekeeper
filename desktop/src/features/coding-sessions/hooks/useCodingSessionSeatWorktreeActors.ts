import * as React from "react";

import {
  getCodingSessionSeatWorktreeActors,
  type CodingSessionSeatWorktreeActors,
} from "@/features/coding-sessions/lib/codingSessionAssignmentInput";

/**
 * Resolve a session's seated actors to the label this host cut their tree
 * under — from the store, not from a relay profile.
 *
 * `useCodingSessionActorNameResolver` answers a *display* name from a kind-0
 * profile, which a seat this app hired by `bee sessions hire` may never have
 * published. The worktree store keys a seat by the label it was cut under
 * (`codingSessionHireSeat.ts`, `worktree.rs`), and since the hire also signs
 * the seat's actor pubkey, the host can now answer **actor → seat label**
 * directly — the one lookup a request to establish a verification input
 * actually needs. A profile name is fine for a byline; it is the wrong
 * source for a question the host's own record can answer exactly.
 */
export type CodingSessionSeatLabelByActor = {
  /** The seat label this host cut a tree under for `actor`, or null. */
  labelForActor: (actor: string) => string | null;
  /**
   * True when this session holds a worktree recorded before this host began
   * tracking actor pubkeys. A caller that gets `null` from
   * {@link labelForActor} while this is true cannot conclude the seat is
   * off-host — it may be a seat this host cut before it could say whose tree
   * it was.
   */
  hasUnattributedSeat: boolean;
};

const EMPTY: CodingSessionSeatWorktreeActors = {
  labels: new Map(),
  unattributed: false,
};

/** Injected only by tests. */
export type CodingSessionSeatWorktreeActorsDeps = {
  fetch: (sessionRef: string) => Promise<CodingSessionSeatWorktreeActors>;
};

const HOST_DEPS: CodingSessionSeatWorktreeActorsDeps = {
  fetch: (sessionRef) => getCodingSessionSeatWorktreeActors(sessionRef),
};

export function useCodingSessionSeatWorktreeActors(
  sessionRef: string | null,
  deps: CodingSessionSeatWorktreeActorsDeps = HOST_DEPS,
): CodingSessionSeatLabelByActor {
  const depsRef = React.useRef(deps);
  depsRef.current = deps;
  const [actors, setActors] =
    React.useState<CodingSessionSeatWorktreeActors>(EMPTY);

  React.useEffect(() => {
    if (sessionRef === null) {
      setActors(EMPTY);
      return;
    }
    let cancelled = false;
    void depsRef.current.fetch(sessionRef).then((answer) => {
      if (!cancelled) setActors(answer);
    });
    return () => {
      cancelled = true;
    };
  }, [sessionRef]);

  return React.useMemo(
    () => ({
      labelForActor: (actor: string) =>
        actors.labels.get(actor.toLowerCase()) ?? null,
      hasUnattributedSeat: actors.unattributed,
    }),
    [actors],
  );
}
