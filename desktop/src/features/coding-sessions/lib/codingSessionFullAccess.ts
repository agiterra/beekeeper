/**
 * Copy for the per-session "full access to this computer" grant.
 *
 * Pure on purpose, so a node test can pin every sentence the header shows.
 * The state it reads is what the host answered (`coding_session_full_access`)
 * — never a guess: a session whose read failed or is not this computer's has
 * no view at all, and so no line here.
 */

/** The `⋯` menu item's label. */
export const CODING_SESSION_FULL_ACCESS_LABEL = "Full access to this computer";

/** The header badge shown while the grant is on. */
export const CODING_SESSION_FULL_ACCESS_BADGE = "Full access";

/** The badge's hover text: what the badge means, in a sentence. */
export const CODING_SESSION_FULL_ACCESS_BADGE_TITLE =
  "Full access to this computer is on for this session: its agent runs outside the project sandbox. Turn it off from the session's ⋯ menu.";

/** What the header knows about one session's grant. */
export type CodingSessionFullAccessView = {
  /** The host's answer: is this session granted full access? */
  granted: boolean;
  /**
   * The change in flight — the value being written and the restart that puts
   * it in force — or null when nothing is changing.
   */
  pending: boolean | null;
  /** Why the last change did not take, verbatim from where it failed. */
  error: string | null;
};

/** The item's second line for a state. */
export function codingSessionFullAccessDetail(
  view: CodingSessionFullAccessView,
): string {
  if (view.pending === true) {
    return "Turning on — restarting the agent outside the sandbox…";
  }
  if (view.pending === false) {
    return "Turning off — restarting the agent inside the sandbox…";
  }
  if (view.error) {
    return `Not changed: ${view.error}`;
  }
  return view.granted
    ? "On — this agent runs outside the sandbox. Turn off to restart it sandboxed."
    : "Let this agent install tools and work outside the project. Restarts the agent.";
}

/**
 * The message for a change that did not take, and whether the grant was put
 * back the way it was.
 *
 * `revertError` is `null` when the grant was restored (or never written), and
 * the restore's own failure otherwise — in which case the grant on disk is
 * the new value and will apply at the session's next start, which the person
 * has to be told rather than left to discover.
 */
export function codingSessionFullAccessFailure(input: {
  granted: boolean;
  cause: string;
  revertError: string | null;
}): string {
  const cause = input.cause.trim() || "the change did not complete";
  if (input.revertError === null) return cause;
  const pending = input.granted
    ? "full access stays granted and applies the next time this agent starts"
    : "full access stays withdrawn and applies the next time this agent starts";
  return `${cause} Restoring the previous setting also failed (${input.revertError.trim() || "no reason given"}), so ${pending}.`;
}
