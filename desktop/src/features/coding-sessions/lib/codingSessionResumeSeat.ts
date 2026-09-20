/**
 * What a seated **reconnect** hands the host, as one testable step.
 *
 * # The gap this closes (finding 85)
 *
 * Finding 84 made launch and hire stage a seat from the project's own pack
 * source — its kind:30624 — instead of whichever copy happened to be installed
 * on this computer. The resume path was left behind: `publishSeatedCoding
 * SessionResume` grew a `projectRef` and a reader, and nothing passed either.
 * So a reconnect of a project-sourced seat silently restaged the local copy,
 * and the only symptom was a seat running different skills than the one that
 * had been running a minute earlier.
 *
 * Two things were missing and both are fixed:
 *
 * 1. **The custody seam had no reader.** The composer's default custody object
 *    was `{ stageSeat, clearSeat }` — no `fetchPackSource` — so even a
 *    `projectRef` would have resolved to `null`. That object now lives in
 *    `codingSessionResumeSeatDeps.ts`, with the reader the launch dialog's
 *    preview and the create path already share.
 * 2. **Nobody knew the project.** It is on the execution's own 44223
 *    (`projectRef`), which every surface that renders a composer already
 *    holds. This module is the assembly, lifted out of the composer so a test
 *    can watch what the publisher receives rather than mounting React to find
 *    out.
 *
 * Deliberately free of relay and Tauri imports — the production seam is a
 * sibling module — so this stays a pure mapping a node test can load.
 *
 * # The second gap this closes (ledger 187)
 *
 * A role granted the agents repository (`team.yml` `workspace.agents_repo`)
 * needs its clone put beside the seat's worktree. The create cut that tree
 * and knew its path; a reconnect does not, and the host refused the whole
 * re-stage for want of it — so the lead of a project's first team session
 * could not be reconnected after a relaunch at all. The composer does hold
 * the execution's provider **session id**, and the host wrote the tree down
 * under exactly that id when it cut it, so the id is what travels. A caller
 * that does know the worktree (the project-agent restart) still passes it and
 * still wins.
 */
import type { CodingSessionSeatCustody } from "./codingSessionSeatedCreate";

/** The execution facts a reconnect restages from. */
export type CodingSessionResumeSeat = {
  /** The seat's actor pubkey, or null for an execution a person created. */
  actorPubkey: string | null;
  /** The role that seat holds on this execution, from its 44223. */
  role: string | null;
  /**
   * The project the execution is filed under, from its 44223.
   *
   * `null` is a standalone session and stages the local copy on purpose. It is
   * never a stand-in for "we did not look" — the value comes from the same
   * signed metadata the composer renders everything else from.
   */
  projectRef: string | null;
  /**
   * The seat's own worktree when the caller knows it, so the new generation
   * is staged under the § 4.9 branch override. Omitted means "not known",
   * which stages main's definition.
   */
  worktree?: string | null;
  /**
   * The execution's provider session id, from its established command target.
   *
   * With it the host resolves the seat's own worktree from its record of the
   * tree it cut, which is what lets a role granted the agents repository be
   * re-staged at all (ledger 187). Omitted means "not known"; a granted role
   * is then refused with what was looked for rather than staged beside a
   * guess.
   */
  sessionId?: string | null;
};

/**
 * The exact argument `publishSeatedCodingSessionResume` is called with.
 *
 * Deliberately not a wrapper around the publish itself: the caller owns the
 * `commandId` (it arms the settle watcher with it before publishing) and owns
 * the publish closure. This owns only the mapping from execution facts to
 * staging inputs — which is the thing that was wrong.
 */
export function buildCodingSessionResumeInput<T>(input: {
  commandId: string;
  seat: CodingSessionResumeSeat;
  publish: () => Promise<T>;
  deps: CodingSessionSeatCustody;
}): {
  commandId: string;
  actorPubkey: string | null;
  actorRole: string | null;
  projectRef: string | null;
  worktree?: string | null;
  sessionId?: string | null;
  publish: () => Promise<T>;
  deps: CodingSessionSeatCustody;
} {
  return {
    commandId: input.commandId,
    actorPubkey: input.seat.actorPubkey,
    actorRole: input.seat.role,
    projectRef: input.seat.projectRef,
    ...(input.seat.worktree ? { worktree: input.seat.worktree } : {}),
    ...(input.seat.sessionId ? { sessionId: input.seat.sessionId } : {}),
    publish: input.publish,
    deps: input.deps,
  };
}
