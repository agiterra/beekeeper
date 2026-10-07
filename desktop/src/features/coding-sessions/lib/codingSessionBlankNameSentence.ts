import type { CodingSessionTitleMode } from "@/shared/api/tauriCodingSessionNaming";

/**
 * The sentence under the founded page's Name field about what a blank field
 * means at Start.
 *
 * Naming after Start belongs to the host (NIP-CSG § Generated title): the
 * provider that runs the founder's first turn may title the session from
 * that first message and sign the title with its own key, as a kind 44252 —
 * never as the founder's 44229. Readers mark such a title "Auto-named"; a
 * name the person types or picks always wins over it. This desktop never
 * publishes a title after Start, in any mode (D9, SV-56).
 *
 * Whether the host titles at all is the session-title mode of the computer
 * that runs the session (Settings → Coding sessions):
 *
 * - `agent` (the default): the host may title it. Not guaranteed, and this
 *   desktop cannot see whether it will — the host may run
 *   `BEEKEEPER_CSP_AUTO_TITLE=off`, the runtime may set `"titleModel": null`, the
 *   host may predate generated titles, or the first message may carry no
 *   text — so the sentence leads with what is certain (untitled) and states
 *   the host title as a possibility.
 * - `my-model`: the host titles nothing; the person's naming model only
 *   suggests a name in this field before Start.
 * - `off`: nothing titles it.
 *
 * The mode read here is *this* computer's. A session whose runtime is on
 * another computer follows that computer's setting, which this desktop
 * cannot read, so it gets the agent wording — the conditional one — plus
 * that disclosure, rather than a guess.
 */
export const CODING_SESSION_BLANK_NAME_SENTENCE =
  "Left blank, it stays untitled unless the agent's computer names it from the first message.";

export const CODING_SESSION_BLANK_NAME_SENTENCE_MY_MODEL =
  "Left blank, it stays untitled; your naming model only suggests a name in this field while you write.";

export const CODING_SESSION_BLANK_NAME_SENTENCE_OFF =
  "Left blank, it stays untitled: session titles are Off on this computer.";

export const CODING_SESSION_BLANK_NAME_ANOTHER_COMPUTER =
  "A session on another computer follows that computer's setting.";

/**
 * Where the selected runtime runs, as far as this desktop can tell.
 *
 * `unknown` is "no target chosen yet" (Start is blocked then) or a caller
 * that does not know; it reads this computer's mode, which is the setting a
 * local target — the bootstrap default — follows.
 */
export type CodingSessionBlankNameTarget =
  | "this-computer"
  | "another-computer"
  | "unknown";

/**
 * What a blank Name means at Start, for this computer's title mode and the
 * selected target.
 *
 * `titleMode: null` is "no settings read" — no host (browser preview, an
 * E2E bridge without a seed) or the read still in flight. That is not an
 * error and not a naming model: it gets the default's wording, which is the
 * conditional one.
 */
export function codingSessionBlankNameSentence(
  input: {
    titleMode?: CodingSessionTitleMode | null;
    target?: CodingSessionBlankNameTarget;
  } = {},
): string {
  const { titleMode = null, target = "unknown" } = input;
  if (target === "another-computer") {
    return `${CODING_SESSION_BLANK_NAME_SENTENCE} ${CODING_SESSION_BLANK_NAME_ANOTHER_COMPUTER}`;
  }
  switch (titleMode) {
    case "my-model":
      return CODING_SESSION_BLANK_NAME_SENTENCE_MY_MODEL;
    case "off":
      return CODING_SESSION_BLANK_NAME_SENTENCE_OFF;
    default:
      return CODING_SESSION_BLANK_NAME_SENTENCE;
  }
}
