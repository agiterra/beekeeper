import type { CodingSessionMissionStateInput } from "./codingSessionMissionInspectorModel";
import type { CodingSessionCatalogRecord } from "./codingSessionTypes";

type SignedSeatStatus = {
  label: string;
  record: CodingSessionCatalogRecord;
};

/**
 * Let canonical team transactions win, then use only explicit provider-signed
 * metadata for waiting/stalled. Age, reachability, and silence are excluded.
 */
export function projectCodingSessionMissionState(input: {
  canonical: CodingSessionMissionStateInput;
  seats: readonly SignedSeatStatus[];
}): CodingSessionMissionStateInput {
  if (input.canonical.kind !== "unknown") return input.canonical;
  const signed = input.seats
    .filter(({ record }) => record.statusEventId !== null)
    .sort(
      (left, right) =>
        (right.record.statusAt ?? 0) - (left.record.statusAt ?? 0) ||
        (left.record.statusEventId ?? "").localeCompare(
          right.record.statusEventId ?? "",
        ),
    );
  const waiting = signed.find(
    ({ record }) => record.status === "waiting_for_input",
  );
  if (waiting?.record.statusEventId) {
    return {
      kind: "waiting-on-person",
      sourceEventId: waiting.record.statusEventId,
      requiredAction: `Reply to ${waiting.label} or provide the requested input.`,
      heldOn: waiting.label,
    };
  }
  const stalled = signed.find(({ record }) =>
    ["failed", "disconnected"].includes(record.status),
  );
  if (stalled?.record.statusEventId) {
    return {
      kind: "stalled",
      sourceEventId: stalled.record.statusEventId,
      detail: `${stalled.label} reported ${stalled.record.status}.`,
    };
  }
  return input.canonical;
}
