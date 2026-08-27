/**
 * Domain-separated, UTF-8 byte-length-prefixed key serialization.
 *
 * Copied verbatim from
 * `desktop/src/features/coding-sessions/lib/codingSessionKeys.ts` plus the
 * per-domain key builders that live beside it. Every identity string the
 * observer compares goes through this, so a field value containing a
 * delimiter can never make two distinct tuples serialize identically. Keys
 * minted here compare byte-for-byte with the ones the provider signs.
 */
import type { CodingSessionTarget } from "./types.ts";

export function encodeStructuredKey(
  domain: string,
  ...fields: readonly string[]
): string {
  const encoder = new TextEncoder();
  const encodedFields = fields
    .map((field) => `${encoder.encode(field).byteLength}:${field}`)
    .join("");
  return `${domain}|${encodedFields}`;
}

/**
 * The wire `cs-target` tag value. Generation is part of the key: each
 * generation is its own stream, and a resume mints generation+1.
 */
export function buildCodingSessionTargetKey(
  target: CodingSessionTarget,
): string {
  return encodeStructuredKey(
    "coding-session/v1",
    target.driver,
    target.instanceId,
    target.sessionId,
    String(target.generation),
  );
}

/** The `csm-key` fence a 44223 carries. */
export function codingSessionMetadataSemanticKey(
  target: CodingSessionTarget,
): string {
  return encodeStructuredKey(
    "coding-session-metadata/v1",
    target.driver,
    target.instanceId,
    target.sessionId,
    String(target.generation),
  );
}

/** The `cst-key` fence a 44225 carries. */
export function codingSessionTranscriptSemanticKey(
  target: CodingSessionTarget,
  eventSeq: number,
): string {
  return encodeStructuredKey(
    "coding-session-transcript/v1",
    target.driver,
    target.instanceId,
    target.sessionId,
    String(target.generation),
    String(eventSeq),
  );
}

/** The single-field `csl-key` a lifecycle 44224 carries. */
export function lifecycleReceiptSemanticKey(commandId: string): string {
  return encodeStructuredKey("coding-session-lifecycle-receipt/v1", commandId);
}

/**
 * The `csl-key` a 44224 actually carries. One create publishes one lifecycle
 * receipt, so those keep the single-field key; one turn publishes up to three,
 * so a turn receipt names its stage too.
 */
export function codingSessionReceiptSemanticKey(
  commandId: string,
  status: string,
  isTurnStatus: boolean,
): string {
  return isTurnStatus
    ? encodeStructuredKey(
        "coding-session-lifecycle-receipt/v1",
        commandId,
        status,
      )
    : lifecycleReceiptSemanticKey(commandId);
}

/** Collision-free execution identity: the target tuple minus `generation`. */
export function buildCodingSessionExecutionKey(
  signerPubkey: string,
  target: CodingSessionTarget,
): string {
  return encodeStructuredKey(
    "coding-session-execution/v1",
    signerPubkey,
    "target",
    target.driver,
    target.instanceId,
    target.sessionId,
  );
}

/** Stable per-(channel, signer, generation) identity for catalog records. */
export function buildCodingSessionGenerationId(
  channelId: string,
  signerPubkey: string,
  target: CodingSessionTarget,
): string {
  return encodeStructuredKey(
    "coding-session-transcript-generation/v1",
    channelId,
    signerPubkey,
    target.driver,
    target.instanceId,
    target.sessionId,
    String(target.generation),
  );
}

/**
 * Presentation scope for one target tuple.
 *
 * Deliberately its own domain rather than the wire `cs-target` key: this one
 * only ever fences tool pairing and turn reconstruction inside one projection
 * pass, and giving it the wire key's name would invite someone to sign it.
 */
export function buildCodingSessionTranscriptScopeKey(
  target: CodingSessionTarget,
): string {
  return encodeStructuredKey(
    "coding-session-transcript-scope/v1",
    target.driver,
    target.instanceId,
    target.sessionId,
    String(target.generation),
  );
}
