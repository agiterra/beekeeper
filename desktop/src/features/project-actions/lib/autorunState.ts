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

/**
 * Whether the tab must still offer "Allow runs on this computer" for this
 * workflow (Brian's 2026-09-25 ruling: creation is consent, so an active
 * grant asks nobody — this control exists only for the two cases that
 * survive it, mirroring `projectVerifySetup.ts`'s `verifyConsentCase`):
 * `"none"` — no active grant for the current hash (a foreign host created
 * the project, or the grant publish failed); `"stale"` — an unrevoked grant
 * exists, but for an earlier hash. `null` (an active grant) means hidden.
 */
export function autorunConsentCase(
  state: AutorunGrantState,
): "not-created" | "changed" | null {
  if (state.kind === "active") return null;
  return state.kind === "stale" ? "changed" : "not-created";
}

/** The one sentence each case above names, worded to match the Overview
 * card's `verifyConsentCaseSentence` for the same two cases. */
export function autorunConsentSentence(
  kind: "not-created" | "changed",
): string {
  return kind === "changed"
    ? "The definition changed since this computer last granted it, so it asks again."
    : "This computer holds no standing grant for this workflow's current definition.";
}
