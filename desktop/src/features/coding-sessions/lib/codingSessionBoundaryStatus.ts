/**
 * Transcript copy for the host's project execution boundary disclosure.
 *
 * Each generation's transcript carries one status item from
 * `execution_scope::boundary_status_item` (`crates/buzz-session-provider`):
 * `execution_boundary_enforced` with the backend that enforced it as
 * `reason`, or `execution_boundary_not_enforced` with a stable reason slug.
 * The host emits "enforced" only after the boundary started around the whole
 * process tree, so this copy states what the host observed; it never infers
 * protection from anything else on the item.
 *
 * Only a slug-shaped `reason` is shown: a known one as prose, an unknown one
 * as its bare slug, anything else (a path, a sentence, a value) not at all.
 */

/** The row title for both boundary statuses. */
export const CODING_SESSION_BOUNDARY_TITLE = "Project boundary";

/** What each boundary status means for this session. */
export const CODING_SESSION_BOUNDARY_STATUSES: ReadonlyMap<string, string> =
  new Map([
    [
      "execution_boundary_enforced",
      "Enforced — this session and every process it starts run inside this project's boundary; other projects' files are outside it",
    ],
    [
      "execution_boundary_not_enforced",
      "Not enforced — this session is not isolated from other projects' files",
    ],
  ]);

/** Reader-facing names for the backends and reason slugs the host emits. */
export const CODING_SESSION_BOUNDARY_REASONS: ReadonlyMap<string, string> =
  new Map([
    ["macos-seatbelt", "macOS Seatbelt"],
    ["no-backend-for-platform", "this platform has no boundary backend"],
  ]);

const REASON_SLUG = /^[a-z0-9][a-z0-9._-]{0,63}$/;

/**
 * The row text for a boundary status, or `undefined` when `status` is not
 * one, so the caller falls through to its other status renderers.
 */
export function codingSessionBoundaryText(
  status: string,
  reason: unknown,
): string | undefined {
  const text = CODING_SESSION_BOUNDARY_STATUSES.get(status);
  if (text === undefined) {
    return undefined;
  }
  if (typeof reason !== "string" || reason.length === 0) {
    return `${text} (no reason given)`;
  }
  if (!REASON_SLUG.test(reason)) {
    return `${text} (unrecognized reason)`;
  }
  return `${text} (${CODING_SESSION_BOUNDARY_REASONS.get(reason) ?? reason})`;
}
