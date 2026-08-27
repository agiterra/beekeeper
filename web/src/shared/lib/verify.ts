/**
 * Signature verification for inbound relay events.
 *
 * Mirrors desktop's `hasValidSignature` (`desktop/src/shared/lib/authors.ts`).
 * The seven signed fields are projected explicitly: callers hand in richer
 * objects (cached rows, projections) and `verifyEvent` recomputes the id from
 * whatever it is given, so verifying an unprojected object would make the
 * answer depend on properties nobody signed.
 */
import { verifyEvent } from "nostr-tools/pure";

/** The seven signed fields of a Nostr event. */
export type VerifiableEvent = {
  id: string;
  pubkey: string;
  created_at: number;
  kind: number;
  tags: string[][];
  content: string;
  sig: string;
};

/** True only when the signature over the seven signed fields checks out. */
export function hasValidSignature(event: VerifiableEvent): boolean {
  try {
    return verifyEvent({
      id: event.id,
      pubkey: event.pubkey,
      created_at: event.created_at,
      kind: event.kind,
      tags: event.tags,
      content: event.content,
      sig: event.sig,
    });
  } catch {
    return false;
  }
}
