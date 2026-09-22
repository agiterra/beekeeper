import { normalizePubkey } from "@/shared/lib/pubkey";

/**
 * What separates a mention label from the key that disambiguates it.
 *
 * Agent names are unique per project rather than per computer (ledger 246), so
 * one channel can hold two agents called `Builder`. The composer files the
 * second under `Builder·<key head>` so the draft text names exactly one of
 * them (`useMentions.registerMentionPubkey`).
 *
 * A middot rather than a space or a bracket, and that is load-bearing:
 * `hasMention` ends a name at a non-name boundary and
 * `snapshotDraftMentionRefs` emits a `p` tag for every bound name the text
 * still contains, so `@Builder (bbbbbbbb)` still contains a matchable
 * `@Builder` and mentioning the second agent would tag the first as well.
 * `@Builder·bbbbbbbb` does not. Pinned by `mentionDisambiguation.test.mjs`,
 * because getting it wrong fails silently.
 */
export const MENTION_DISAMBIGUATOR = "·";

/** How much of the key the label carries. Enough to tell two apart at a glance. */
export const MENTION_DISAMBIGUATOR_KEY_CHARS = 8;

/**
 * The label a mention should be filed under, given what this draft already
 * binds.
 *
 * The draft's mention map is keyed by name — that is what lets
 * `snapshotDraftMentionRefs` find each mention in the text and emit its `p`
 * tag. Two agents in one channel can now share a display name, so binding the
 * bare name would have the second registration overwrite the first: the
 * message would go to the wrong Builder, with no error anywhere. When the name
 * is already bound to a *different* identity, the second is filed under
 * `Builder\u00b7<key head>` instead, and the caller inserts that label so the
 * text and the wire agree about who was meant.
 */
export function mentionLabelFor(
  name: string,
  pubkey: string,
  bound: ReadonlyMap<string, string>,
): string {
  const held = bound.get(name);
  const key = normalizePubkey(pubkey);
  if (held === undefined || normalizePubkey(held) === key) return name;
  return `${name}${MENTION_DISAMBIGUATOR}${key.slice(
    0,
    MENTION_DISAMBIGUATOR_KEY_CHARS,
  )}`;
}
