/**
 * The shape of a managed agent a seat field actually needs.
 *
 * Structural on purpose. `ManagedAgent` (`@/shared/api/types`) satisfies it
 * today and gains `homeRole`/`hasRolePack` when the installer that mints role
 * packs into agents lands — so the seat UI can consume those fields without
 * either side depending on the other's branch.
 *
 * Both new fields are optional, and `undefined` is a distinct answer from
 * `null`/`false`: it means *this build never asked*. A field rendered from
 * `undefined` would accuse an agent of lacking a role pack nobody looked for.
 */
export type CodingSessionSeatAgent = {
  /** Lowercase 64-hex public key of the managed agent. */
  pubkey: string;
  /** Display name, as the agents list shows it. */
  name: string;
  /** `"running"` when a process is live; anything else reads as stopped. */
  status?: string;
  /**
   * The role this agent *is*, from its pack persona. `null` when it declares
   * none; `undefined` when this build cannot tell.
   */
  homeRole?: string | null;
  /**
   * Whether this computer can stage a role pack for it. `false` means a seat
   * on this agent runs on its persona prompt alone; `undefined` means unknown.
   */
  hasRolePack?: boolean;
};
