import type { ProjectCodingSessionShelfEntry } from "./projectCodingSessionShelf";

/**
 * Metadata-only shelf wording. The shelf does not read session leases, so it
 * may report what the provider last said but may never present that report as
 * current liveness.
 */
export function projectSessionObservationLabel(
  status: ProjectCodingSessionShelfEntry["status"],
): string {
  return `Reported ${status.label.toLowerCase()}`;
}
