/**
 * Relay *identity* (as opposed to relay *address*) reconciliation.
 *
 * Every persisted client cache — channel snapshots, message snapshots, the
 * Rust retention database, `archive.db` — is keyed by the relay **URL**. A
 * relay reinstalled at the same URL with a fresh signing keypair is a
 * different community that happens to answer at the same address, and without
 * a check the client silently reuses the previous instance's cached scope.
 *
 * The relay advertises its own signing key in its NIP-11 document as `self`.
 * This module turns "what we stored" + "what the relay says now" into a single
 * action. It is deliberately **pure and import-free** so the decision matrix
 * is exhaustively unit-testable; all side effects live in
 * `relayIdentityGuard.ts`.
 *
 * ## Failure posture
 *
 * A false-positive mismatch destroys user data (it wipes every relay-scoped
 * cache); a missed mismatch merely reproduces today's behaviour. So every
 * ambiguous input — unreachable relay, request error, relay that advertises no
 * `self`, malformed hex — resolves to `none`. Only a *resolved, well-formed,
 * different* observation can trigger a wipe.
 */

/** Lowercase 32-byte hex — the only shape a Nostr pubkey may take here. */
const RELAY_PUBKEY_PATTERN = /^[0-9a-f]{64}$/;

/**
 * What the NIP-11 probe found.
 *
 * - `resolved` + `pubkey: string` — the relay advertised a `self` value.
 * - `resolved` + `pubkey: null` — the relay answered but advertises no `self`
 *   (a valid answer, and *not* evidence of a different relay).
 * - `unresolved` — unreachable, timed out, errored, or never attempted.
 */
export type RelayIdentityObservation =
  | { status: "resolved"; pubkey: string | null }
  | { status: "unresolved" };

/**
 * Why no action is taken. Distinguished so callers can log the real reason
 * instead of a generic "skipped".
 */
export type RelayIdentityNoActionReason =
  /** Probe failed, timed out, or was never attempted. */
  | "unresolved"
  /** Relay answered but advertises no NIP-11 `self`. */
  | "unadvertised"
  /** Relay advertised something that is not a lowercase 64-char hex key. */
  | "invalid-observed"
  /** Stored key equals observed key — same relay instance. */
  | "match";

export type RelayIdentityAction =
  | { kind: "none"; reason: RelayIdentityNoActionReason }
  /**
   * No usable stored identity yet (fresh community, or a record written by a
   * build that predates this field). Record the observed key; touch nothing
   * else. This is the upgrade path for every existing install.
   */
  | { kind: "adopt"; relayPubkey: string }
  /**
   * A usable stored identity exists and the relay is now signing with a
   * different key: a new relay instance at a familiar URL. Evict every
   * relay-URL-scoped cache, then re-key to the observed identity.
   */
  | {
      kind: "rekey";
      relayPubkey: string;
      previousRelayPubkey: string;
    };

/**
 * Canonicalize a relay `self` pubkey, or `null` when the value cannot be a
 * pubkey at all. Case and surrounding whitespace are not identity.
 */
export function normalizeRelayPubkey(
  value: string | null | undefined,
): string | null {
  if (typeof value !== "string") {
    return null;
  }
  const normalized = value.trim().toLowerCase();
  return RELAY_PUBKEY_PATTERN.test(normalized) ? normalized : null;
}

/**
 * Decide what to do about the active community's relay identity.
 *
 * Pure: no storage, no network, no clock. See the module doc for the failure
 * posture — every branch that is not "resolved, well-formed, and different
 * from a well-formed stored key" returns `none`.
 *
 * A *stored* value that fails normalization (corrupt localStorage) is treated
 * as absent and therefore `adopt`, never `rekey` — garbage in the record must
 * not be read as proof the relay changed.
 */
export function decideRelayIdentityAction(input: {
  /** `Community.relayPubkey` as persisted, if any. */
  storedRelayPubkey?: string | null;
  observation: RelayIdentityObservation;
}): RelayIdentityAction {
  const { observation, storedRelayPubkey } = input;

  if (observation.status !== "resolved") {
    return { kind: "none", reason: "unresolved" };
  }
  if (observation.pubkey === null) {
    return { kind: "none", reason: "unadvertised" };
  }

  const observed = normalizeRelayPubkey(observation.pubkey);
  if (observed === null) {
    return { kind: "none", reason: "invalid-observed" };
  }

  const stored = normalizeRelayPubkey(storedRelayPubkey);
  if (stored === null) {
    return { kind: "adopt", relayPubkey: observed };
  }
  if (stored === observed) {
    return { kind: "none", reason: "match" };
  }

  return {
    kind: "rekey",
    relayPubkey: observed,
    previousRelayPubkey: stored,
  };
}
