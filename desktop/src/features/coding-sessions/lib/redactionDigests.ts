/**
 * Which redaction digests a transcript actually contains, and whose machine
 * would be able to resolve them.
 *
 * A published marker carries a digest and nothing else, so recovering the value
 * behind it means asking *this* machine what it removed. That is only a
 * sensible question when this machine is the one that redacted it — hence
 * `signerPubkey` and `sessionId` travel with the digests rather than being
 * assumed.
 */

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import { REDACTION_MARKER_PATTERN } from "@/shared/lib/redactionMarker";

/** Ceiling on one lookup, matching the Tauri command's own bound. */
export const MAX_REDACTION_DIGESTS = 512;

/** Everything one lookup needs, or `null` when there is nothing to ask. */
export type RedactionLookupScope = {
  digests: string[];
  sessionId: string;
  signerPubkey: string;
};

/**
 * Collect the distinct redaction digests in `items`, with the session and
 * signer they belong to.
 *
 * Returns `null` — meaning "do not ask" — when the transcript carries no
 * marker, or when it does not name a single signer and session. A transcript
 * with items from two signers is not a state the provider produces; refusing
 * it is cheaper than picking one and being wrong about which machine to ask.
 */
export function collectRedactionLookupScope(
  items: readonly TranscriptItem[],
): RedactionLookupScope | null {
  const digests: string[] = [];
  const seen = new Set<string>();
  let sessionId: string | null = null;
  let signerPubkey: string | null = null;

  for (const item of items) {
    for (const text of redactableStrings(item)) {
      REDACTION_MARKER_PATTERN.lastIndex = 0;
      for (
        let match = REDACTION_MARKER_PATTERN.exec(text);
        match !== null;
        match = REDACTION_MARKER_PATTERN.exec(text)
      ) {
        const digest = match[2];
        if (seen.has(digest) || seen.size >= MAX_REDACTION_DIGESTS) continue;
        seen.add(digest);
        digests.push(digest);
      }
    }
    if (digests.length === 0) continue;
    const itemSession = item.sessionId ?? null;
    const itemSigner = item.bridgeSource?.pubkey ?? null;
    if (itemSession) {
      if (sessionId !== null && sessionId !== itemSession) return null;
      sessionId = itemSession;
    }
    if (itemSigner) {
      if (signerPubkey !== null && signerPubkey !== itemSigner) return null;
      signerPubkey = itemSigner;
    }
  }

  if (digests.length === 0 || !sessionId || !signerPubkey) return null;
  // Sorted so an unchanged transcript produces an identical query key however
  // its items were ordered on the way in.
  return { digests: digests.sort(), sessionId, signerPubkey };
}

/**
 * The strings on one item that can carry a marker.
 *
 * Deliberately the same set the renderers put through the pill: prose, titles,
 * tool arguments, and tool output. Asking for a digest that is never rendered
 * would resolve a value nothing displays.
 */
function redactableStrings(item: TranscriptItem): string[] {
  const strings: string[] = [];
  if ("title" in item && typeof item.title === "string")
    strings.push(item.title);
  if ("text" in item && typeof item.text === "string") strings.push(item.text);
  if (item.type === "tool") {
    if (item.result) strings.push(item.result);
    try {
      strings.push(JSON.stringify(item.args));
    } catch {
      // A tool input that will not serialize cannot be rendered either.
    }
    if (item.descriptor?.preview) strings.push(item.descriptor.preview);
    if (item.descriptor?.label) strings.push(item.descriptor.label);
    if (item.descriptor?.action?.object) {
      strings.push(item.descriptor.action.object);
    }
  }
  return strings;
}
