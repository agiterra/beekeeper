/**
 * Who drove a coding-session turn, and what to call them.
 *
 * A coding session is multi-operator: the founder plus any granted operators
 * can each send turns into the same execution. The provider stamps the
 * verified commanding signer onto every `user_prompt` item it publishes
 * (`operatorPubkey`), so the transcript can name the operator instead of
 * assuming every user message belongs to whoever happens to be reading it.
 *
 * Pure on purpose — the renderer supplies the local identity and the profile
 * lookup it already has, and nothing here touches React or the network.
 */

import {
  resolveUserLabel,
  type UserProfileLookup,
} from "@/features/profile/lib/identity";

const LOWERCASE_HEX_PUBKEY = /^[0-9a-f]{64}$/;

/**
 * Read an operator pubkey off an untrusted value, or `null`.
 *
 * Case is folded before the shape check so a provider that published
 * uppercase hex still attributes correctly; anything that is not 64 hex
 * characters is discarded rather than displayed as a mystery string.
 */
export function normalizeOperatorPubkey(value: unknown): string | null {
  if (typeof value !== "string") {
    return null;
  }
  const normalized = value.trim().toLowerCase();
  return LOWERCASE_HEX_PUBKEY.test(normalized) ? normalized : null;
}

/**
 * The label to show above a user message in a coding-session transcript.
 *
 * - no `operatorPubkey` (items from before attribution existed) -> `"You"`,
 *   because there is no fact to contradict the historical rendering and
 *   inventing a name would be worse than the status quo;
 * - the local operator -> `"You"`, unchanged;
 * - any other operator -> their display name, their NIP-05 handle, or a
 *   truncated pubkey — whichever is the most specific thing actually known.
 *
 * When the local identity has not loaded yet, a stamped prompt resolves to the
 * operator's own label rather than `"You"`: naming the operator is true for
 * every viewer, while `"You"` would be a guess about who is reading.
 */
export function resolveCodingSessionPromptAuthorLabel(input: {
  operatorPubkey?: string | null;
  currentUserPubkey?: string | null;
  profiles?: UserProfileLookup;
}): string {
  const operatorPubkey = normalizeOperatorPubkey(input.operatorPubkey);
  if (!operatorPubkey) {
    return "You";
  }
  const currentUserPubkey = normalizeOperatorPubkey(input.currentUserPubkey);
  return resolveUserLabel({
    currentPubkey: currentUserPubkey ?? undefined,
    profiles: input.profiles,
    pubkey: operatorPubkey,
  });
}

/**
 * The distinct foreign operators appearing in `items`, sorted for a stable
 * reference across renders.
 *
 * Only foreign operators are returned: the local user's own profile never
 * needs resolving, because they render as `"You"`. The result feeds the batch
 * profile query, so a transcript driven by one operator issues one lookup no
 * matter how many turns it contains.
 */
export function collectForeignOperatorPubkeys(
  items: readonly unknown[],
  currentUserPubkey: string | null | undefined,
): string[] {
  const local = normalizeOperatorPubkey(currentUserPubkey);
  const found = new Set<string>();
  for (const item of items) {
    // Read positionally rather than by transcript-item type: only user
    // messages carry the field, and every other variant simply has no
    // `operatorPubkey` to find.
    const operatorPubkey =
      typeof item === "object" && item !== null
        ? normalizeOperatorPubkey(
            (item as { operatorPubkey?: unknown }).operatorPubkey,
          )
        : null;
    if (operatorPubkey && operatorPubkey !== local) {
      found.add(operatorPubkey);
    }
  }
  return [...found].sort();
}
