import { invoke, isTauri } from "@tauri-apps/api/core";

/**
 * One event's native verification result (`verify_event_signatures`,
 * `desktop/src-tauri/src/commands/event_signatures.rs`).
 *
 * - `valid`: the id is the NIP-01 hash of the fields sent and the signature
 *   verifies over it.
 * - `invalid-signature`: the id is the hash of the fields sent; the signature
 *   does not verify.
 * - `unchecked`: not verified — the id does not match the fields, or the event
 *   could not be read. Not a verdict about the (id, sig) pair.
 */
export type NativeSignatureVerdict =
  | "valid"
  | "invalid-signature"
  | "unchecked";

/** The seven signed NIP-01 fields, as sent to the native verifier. */
export type SignedEventFields = {
  id: string;
  pubkey: string;
  created_at: number;
  kind: number;
  tags: string[][];
  content: string;
  sig: string;
};

/** Verifies a batch; one verdict per event, in input order. */
export type NativeSignatureVerifier = (
  events: SignedEventFields[],
) => Promise<NativeSignatureVerdict[]>;

/**
 * The packaged app's native batch verifier, or `null` outside Tauri (the E2E
 * mock bridge and node tests), where callers verify in JavaScript instead.
 */
export function nativeSignatureVerifier(): NativeSignatureVerifier | null {
  if (!isTauri()) return null;
  return (events) =>
    invoke<NativeSignatureVerdict[]>("verify_event_signatures", { events });
}
