import type { ProjectCodingSessionShelfEntry } from "./projectCodingSessionShelf";

/**
 * Metadata-only shelf wording. The shelf does not read session leases, so it
 * may report what the provider last said but may never present that report as
 * current liveness.
 *
 * The visible label is the bare status — "Working", "Idle" — because a row of
 * "Reported working" reads as noise next to every sibling row saying the same
 * word twice. The provenance that word does *not* carry moves to
 * {@link projectSessionObservationTitle}, which the row hangs on a hover, so
 * the distinction stays in the product rather than being deleted from it.
 */
export function projectSessionObservationLabel(
  status: ProjectCodingSessionShelfEntry["status"],
): string {
  return status.label;
}

/** Where the shelf's status word came from, and what it does not prove. */
export function projectSessionObservationTitle(
  status: ProjectCodingSessionShelfEntry["status"],
): string {
  return `${status.label} is what this session's provider last reported. The shelf reads signed metadata, not a live lease, so it does not prove the provider is answering right now.`;
}
