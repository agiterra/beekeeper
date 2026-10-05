/**
 * Where each Pulse session's name came from, kept beside the digest.
 *
 * The digest's bytes are pinned by `conformance/project-pulse-fold`, so a
 * generated title (44252) cannot announce itself inside the session object.
 * The shared fold already reports it in `nameOriginsBySession`; this table
 * carries that answer from {@link foldProjectPulseDigest} to the surface
 * without adding a key the Rust fold would not write, so a Pulse card can mark
 * a model's words "Auto-named" rather than present them as a person's name.
 *
 * Keyed by digest identity. The digest query opts out of React Query's
 * structural sharing so the object a reader holds is the object the fold
 * returned (`pulseQueries.ts`). A digest with no entry here — one built by a
 * test, or decoded from somewhere else — reads as "origin unknown", which
 * shows no marker; it never invents one.
 *
 * Community-scoped: `resetProjectPulseNameOrigins()` is called from
 * `resetProjectPulseState()`, which `resetCommunityState()` runs.
 *
 * Import constraint: relative `.ts` specifiers and erasable TypeScript only —
 * `pulseFold.ts` imports this file under plain `node --test`.
 */
import type { CoordinatedSessionNameOrigin } from "../../../shared/coordination/sessionCoordinationNames.ts";

type NameOrigins = ReadonlyMap<string, CoordinatedSessionNameOrigin>;

let originsByDigest = new WeakMap<object, NameOrigins>();

const NO_ORIGINS: NameOrigins = new Map();

/** Record the fold's name origins for one digest object. */
export function rememberPulseNameOrigins(
  digest: object,
  origins: NameOrigins,
): void {
  originsByDigest.set(digest, origins);
}

/**
 * The name origins the fold reported for this digest, keyed by `sessionKey`.
 * Empty when the digest did not come from this client's fold.
 */
export function pulseNameOrigins(digest: object | null): NameOrigins {
  if (digest === null) return NO_ORIGINS;
  return originsByDigest.get(digest) ?? NO_ORIGINS;
}

/** Drop every recorded origin (community switch). */
export function resetProjectPulseNameOrigins(): void {
  originsByDigest = new WeakMap();
}
