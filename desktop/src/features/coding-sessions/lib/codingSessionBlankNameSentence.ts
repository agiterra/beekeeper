/**
 * The sentence under the founded page's Name field about what a blank field
 * means at Start.
 *
 * Naming after Start belongs to the host now (NIP-CSG § Generated title):
 * the provider that runs the founder's first turn may title the session from
 * that first message and sign the title with its own key, as a kind 44252 —
 * never as the founder's 44229. Readers mark such a title "Auto-named"; a
 * name the person types or picks always wins over it.
 *
 * The title is not guaranteed, and this desktop cannot see whether it will
 * come: the host may run `BUZZ_CSP_AUTO_TITLE=off`, the runtime may set
 * `"titleModel": null`, the host may predate generated titles, or the first
 * message may carry no text. The runtime catalog does not advertise any of
 * that, so the sentence leads with what is certain — the session stays
 * untitled — and states the host title as a possibility, not a promise.
 */
export const CODING_SESSION_BLANK_NAME_SENTENCE =
  "Left blank, it stays untitled unless the agent's computer names it from the first message.";

/** What a blank Name means at Start: untitled unless the host titles it. */
export function codingSessionBlankNameSentence(): string {
  return CODING_SESSION_BLANK_NAME_SENTENCE;
}
