/**
 * The Actions tab's autorun badge, as a pure read of a workflow's grants.
 *
 * Ledger 252, control run 6: project setup used to manufacture a whole run
 * just to have something for its "Approve and allow future runs" click to
 * answer. Now that a grant can stand with no run at all
 * (`grant_standing_approval`, `handle_standing_approval_grant` in
 * `crates/buzz-relay/src/handlers/command_executor.rs`), the tab must say so
 * honestly — "autorun" next to zero runs must never read as "approved run".
 */

/** The one fact this reads off each grant. */
export type AutorunGrantLike = {
  revokedAt: string | null;
  matchesCurrent: boolean;
  grantedBy: string;
};

/** Which of a workflow's grants (if any) is live against its current definition. */
export type AutorunGrantState =
  | { kind: "active"; grantedBy: string }
  | { kind: "stale" }
  | { kind: "none" };

/**
 * The active grant, an unrevoked grant for a now-stale hash, or none.
 *
 * An active grant always wins over a stale one — an edited-then-reverted
 * definition can carry both, and the exact-hash match is what the relay
 * itself gates runs on.
 */
export function autorunGrantState(
  grants: readonly AutorunGrantLike[] | null | undefined,
): AutorunGrantState {
  const active = (grants ?? []).find(
    (grant) => grant.revokedAt === null && grant.matchesCurrent,
  );
  if (active) return { kind: "active", grantedBy: active.grantedBy };
  const stale = (grants ?? []).some(
    (grant) => grant.revokedAt === null && !grant.matchesCurrent,
  );
  return stale ? { kind: "stale" } : { kind: "none" };
}

/**
 * The note beside an active-grant badge when the workflow has never run:
 * "standing grant · no run yet" rather than letting the badge alone imply an
 * approved run happened. `null` once any run exists, or when there is no
 * active grant to caveat.
 */
export function standingGrantNote(
  state: AutorunGrantState,
  runCount: number,
): string | null {
  if (state.kind !== "active") return null;
  return runCount === 0 ? "standing grant · no run yet" : null;
}
