export type Community = {
  id: string;
  name: string;
  relayUrl: string;
  token?: string;
  /**
   * The pubkey associated with the active identity at the time the community
   * was created. Display-only — auth always uses the persisted `identity.key`
   * file resolved at startup, never this field.
   */
  pubkey?: string;
  /**
   * The relay's *own* signing key (NIP-11 `self`, lowercase hex) as observed
   * the last time this community connected — the relay's identity, as opposed
   * to `relayUrl` which is only its address.
   *
   * Optional and nullable by design: records written before this field existed
   * have none, and a relay may legitimately advertise no `self`. Absent means
   * "unknown", never "mismatch" — see `relayIdentity.ts`. It is adopted on the
   * next connect that resolves a well-formed value.
   */
  relayPubkey?: string;
  addedAt: string;
  /**
   * Absolute directory the agent's `~/.buzz/REPOS` symlinks to, so agents
   * work in the user's existing checkouts instead of re-cloning. `~` is
   * expanded to an absolute path before save. Unset = the default real
   * `REPOS` directory inside the nest.
   */
  reposDir?: string;
  /**
   * @deprecated Never read. Kept on the type so old localStorage entries
   * deserialise without errors. New entries never set this field, and
   * `loadCommunities()` strips it on read so it cannot leak forward. The
   * authoritative private key is the on-disk `identity.key` file.
   */
  nsec?: never;
};
