import { requestCodingSessionUmbrellaItemReveal } from "./CodingSessionUmbrellaTimelineWindow";

/**
 * Scroll the parent conversation to a subagent's row after its page closes.
 *
 * Finds the row by the call's item id: a per-spawn element marked
 * `data-subagent-call-id`, else the group row whose `data-subagent-call-ids`
 * names it. Unlike `revealCodingSessionSubagentInStream` it does not open the
 * group: returning from the page should land on the row, not unfold it. A row
 * the DOM does not hold (above the umbrella's window) is asked for through the
 * umbrella timeline's reveal event. Returns whether anything owned it.
 */
export function revealCodingSessionSubagentRow(callItemId: string): boolean {
  if (typeof document === "undefined") return false;
  const id =
    typeof CSS !== "undefined" && CSS.escape
      ? CSS.escape(callItemId)
      : callItemId.replace(/["\\]/g, "\\$&");
  const row = document.querySelector(
    `[data-subagent-call-id="${id}"], [data-subagent-call-ids~="${id}"]`,
  );
  if (row instanceof HTMLElement) {
    row.scrollIntoView({ behavior: "auto", block: "center" });
    return true;
  }
  return requestCodingSessionUmbrellaItemReveal(callItemId);
}
