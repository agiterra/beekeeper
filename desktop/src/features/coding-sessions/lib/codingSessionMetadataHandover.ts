// The `handover` key of a kind-44223 metadata event: the fence disclosure
// `SessionMetadataHandover` in `crates/beekeeper-core/src/coding_session_payload.rs`.
// Split out of `codingSessionIngressPayloads.ts` (which re-exports the type)
// to keep that decoder under the repository file-size ceiling.
import { hasExactKeys, isPlainRecord } from "./codingSessionWireDecode";

/** The fence disclosure a 44223 carries: `SessionMetadataHandover` exactly. */
export type CodingSessionMetadataHandover = {
  /** `active` while the claim stands; `voided` once the claimant lost it. */
  state: "active" | "voided";
  claimant: string;
  bodyPubkey: string;
  acceptedEventId: string;
};

const HEX64 = /^[0-9a-f]{64}$/;

/**
 * Read a `handover`, or null for anything that is not exactly one.
 *
 * Four keys, all required, each a lowercase 64-hex id, and a two-token
 * `state` — `SessionMetadataHandover` in
 * `crates/beekeeper-core/src/coding_session_payload.rs` is `deny_unknown_fields`
 * and every field is required. There is no `none` token: absence of the whole
 * key is how a provider says no claim stands, so a partial object is refused
 * rather than read loosely.
 */
export function readMetadataHandover(
  value: unknown,
): Readonly<CodingSessionMetadataHandover> | null {
  if (
    !isPlainRecord(value) ||
    !hasExactKeys(value, [
      "state",
      "claimant",
      "bodyPubkey",
      "acceptedEventId",
    ]) ||
    (value.state !== "active" && value.state !== "voided") ||
    typeof value.claimant !== "string" ||
    !HEX64.test(value.claimant) ||
    typeof value.bodyPubkey !== "string" ||
    !HEX64.test(value.bodyPubkey) ||
    typeof value.acceptedEventId !== "string" ||
    !HEX64.test(value.acceptedEventId)
  ) {
    return null;
  }
  return Object.freeze({
    state: value.state,
    claimant: value.claimant,
    bodyPubkey: value.bodyPubkey,
    acceptedEventId: value.acceptedEventId,
  });
}
