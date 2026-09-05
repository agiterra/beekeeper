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
  publish: () => Promise<T>;
  deps: CodingSessionSeatCustody;
} {
  return {
    commandId: input.commandId,
    actorPubkey: input.seat.actorPubkey,
    actorRole: input.seat.role,
    projectRef: input.seat.projectRef,
    publish: input.publish,
    deps: input.deps,
  };
}
