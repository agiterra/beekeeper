/**
 * SV-35: the words the composer's model chips say, from the switch fold and
 * the execution's own facts. At most three lines, so a disclosure never grows
 * into a paragraph inside a chip's popover.
 *
 * The rules (spec § Desktop, "Honesty labels"):
 *
 * - pending: "Switching to X at the next turn" until the provider answers;
 * - applied: the model is metadata's, and a mismatch says "Asked X · running Y";
 * - refused: the provider's reason, and the chip back on metadata's model;
 * - no `modelSwitch`: the provider cannot do it, said as such;
 * - a viewer who may not control the session: who can.
 *
 * Effort has no turn-level evidence (WIRE-C3a § 5): an `[effort]` in metadata
 * is the adapter's acknowledgement, so a line names it as that, never as what
 * a turn ran at.
 */
import type { CodingSessionModelSwitchState } from "./codingSessionModelSwitch";

/** Whether the chips may switch, or why they only show. */
export type CodingSessionModelSwitchAvailability =
  | "available"
  | "unsupported"
  | "not-controller";

/** The sentence a display-only chip shows in place of the old "fixed" copy. */
export const CODING_SESSION_MODEL_SWITCH_UNSUPPORTED_TEXT =
  "This execution's provider cannot change models mid-session.";

/** The sentence a viewer who may not steer reads. */
export const CODING_SESSION_MODEL_SWITCH_OPERATORS_ONLY_TEXT =
  "Only people who can control this session can change its model.";

/** The effort disclosure: acknowledged by the adapter, never observed. */
export const CODING_SESSION_MODEL_SWITCH_EFFORT_TEXT =
  "Effort is the runtime's acknowledgement; no turn reports the effort it ran at.";

const MAX_ROWS = 3;

/** The provider's refusal codes in a person's words; anything else, its own. */
function refusalSentence(
  state: Extract<CodingSessionModelSwitchState, { kind: "refused" }>,
  label: (selection: string) => string,
): string {
  switch (state.code) {
    case "MODEL_SWITCH_UNSUPPORTED":
      return `Not switched: ${CODING_SESSION_MODEL_SWITCH_UNSUPPORTED_TEXT}`;
    case "MODEL_NOT_OFFERED":
      return `Not switched: this provider does not offer ${label(state.requested)} here.`;
    case "MODEL_SWITCH_FAILED":
      return "Not switched: the runtime refused the change and kept its previous model.";
    default:
      return state.outcome === "dropped"
        ? `Not switched: the provider dropped the request (${state.code}).`
        : `Not switched: ${state.message}`;
  }
}

/**
 * The note shown beside the chips, or `null` when there is nothing to say.
 *
 * `label` turns a selection into its display form (`Sonnet 5 · High`); it is
 * injected so this file stays free of the catalog and its names.
 */
export function codingSessionModelSwitchNote(
  state: CodingSessionModelSwitchState,
  label: (selection: string) => string,
): { tone: "muted" | "warning"; text: string } | null {
  switch (state.kind) {
    case "idle":
      return null;
    case "pending":
      return {
        tone: "muted",
        text: `Switching to ${label(state.requested)} at the next turn`,
      };
    case "accepted":
      return {
        tone: "muted",
        text: `Switch to ${label(state.requested)} accepted; waiting for the provider's record of the model in effect`,
      };
    case "applied":
      return state.matches
        ? null
        : {
            tone: "warning",
            text: `Asked ${label(state.requested)} · running ${
              state.running === null ? "an unnamed model" : label(state.running)
            }`,
          };
    case "refused":
      return { tone: "warning", text: refusalSentence(state, label) };
  }
}

/**
 * The popover's disclosure lines (at most three), in reading order.
 *
 * `effectiveHasEffort` is whether metadata's model carries an effort token;
 * only then is the effort disclosure owed.
 */
export function codingSessionModelSwitchRows(input: {
  availability: CodingSessionModelSwitchAvailability;
  state: CodingSessionModelSwitchState;
  effectiveHasEffort: boolean;
  label: (selection: string) => string;
}): string[] {
  const rows: string[] = [];
  if (input.availability === "unsupported") {
    rows.push(CODING_SESSION_MODEL_SWITCH_UNSUPPORTED_TEXT);
  } else if (input.availability === "not-controller") {
    rows.push(CODING_SESSION_MODEL_SWITCH_OPERATORS_ONLY_TEXT);
  } else {
    rows.push(
      "A change applies at the next turn boundary; the model shown is the one the provider records.",
    );
    const note = codingSessionModelSwitchNote(input.state, input.label);
    if (note) rows.push(note.text);
  }
  if (input.effectiveHasEffort)
    rows.push(CODING_SESSION_MODEL_SWITCH_EFFORT_TEXT);
  return rows.slice(0, MAX_ROWS);
}
