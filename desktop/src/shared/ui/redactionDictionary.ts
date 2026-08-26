import * as React from "react";

/**
 * What this machine remembers about a value it redacted out of its own
 * transcripts.
 *
 * Only ever populated for a session **this desktop's provider produced**, and
 * only ever with classes that are private rather than secret — a host path or
 * opaque provider bookkeeping. The gate lives in `buzz-core`, where the
 * redaction is decided; nothing here can widen it.
 */
export type ResolvedRedaction = {
  plaintext: string;
  /** `host-path` or `structural`. */
  class: string;
};

/**
 * Digest → plaintext, for the transcript currently on screen.
 *
 * A context rather than a prop because the pill that needs it is produced deep
 * inside cached markdown element trees (see `markdown/nodeCache.ts`): a context
 * is read at render time, so a tree cached across mounts still sees the current
 * dictionary, while a prop baked into a cached element would go stale.
 *
 * **An absent digest is not a claim.** "Never recorded", "expired", and "this
 * is not the machine that produced it" are indistinguishable here, so consumers
 * render the same unresolved pill for all three rather than inventing a label
 * for a state they cannot prove.
 */
export const RedactionDictionaryContext = React.createContext<
  ReadonlyMap<string, ResolvedRedaction>
>(new Map());

/** Look up one digest in the dictionary currently in scope. */
export function useResolvedRedaction(
  digest: string,
): ResolvedRedaction | undefined {
  return React.useContext(RedactionDictionaryContext).get(digest);
}
