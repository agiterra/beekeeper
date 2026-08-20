import { invokeTauri } from "@/shared/api/tauri";

/**
 * React Query cache key for the relay's NIP-11 `self` pubkey.
 *
 * Deliberately **not** scoped by relay URL: several call sites read it with an
 * exact-key `getQueryData`/`getQueryState`, so the key shape is load-bearing.
 * The value is therefore community-scoped state living under a global key, and
 * it is dropped explicitly on every community switch by `resetCommunityState`
 * (see `features/communities/useCommunityInit.ts`) so community A's relay
 * identity can never be served to community B.
 *
 * Declared here rather than in `../hooks` so community code can reference it
 * without pulling the whole moderation hook surface; `../hooks` re-exports it.
 */
export const relaySelfQueryKey = ["relaySelf"] as const;

/**
 * Read the active relay's NIP-11 `self` pubkey (its own signing key, hex), or
 * `null` when the relay advertises none or an invalid key. Network and malformed
 * document failures reject the request. Callers that use this value to trust
 * relay-signed state must treat both `null` and errors as untrusted.
 */
export function getRelaySelf(): Promise<string | null> {
  return invokeTauri<string | null>("get_relay_self");
}
