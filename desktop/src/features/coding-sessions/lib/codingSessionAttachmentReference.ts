/**
 * Whether a prompt's prose already points at the attachments it carries.
 *
 * Its own module rather than a helper inside `codingSessionTranscriptItems.ts`:
 * that file sits on the 1000-line ceiling, and this is a self-contained
 * question about one string that reads better beside its own tests anyway.
 */

/**
 * The two markdown forms a turn's own text uses to reference an attachment.
 *
 * An image is `![alt](…)` and renders as the picture itself; a pasted file is a
 * plain `[name](…/media/<64 hex>.…)` and renders as a link the reader can
 * follow. Requiring the content-addressed path on the plain form is what keeps
 * an ordinary link someone typed in their prose from counting — `[the
 * docs](http://example.com)` references nothing this turn carried.
 *
 * Both are written by the composer at send time
 * (`useCodingSessionTurnAttachments`' `expandAttachmentTokens`), and both are
 * what the provider splits the prompt on, so this predicate is asking the same
 * question the agent's own reader asks.
 */
export function containsAttachmentReference(content: string): boolean {
  return (
    /!\[[^\]]*\]\([^)]+\)/.test(content) ||
    /\[[^\]]*\]\([^)]*\/media\/[0-9a-f]{64}[^)]*\)/.test(content)
  );
}
