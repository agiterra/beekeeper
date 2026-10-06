/**
 * The trust gate: which observed events this browser may believe, and on
 * whose authority (D5).
 *
 * Mirrors `classifyTrustedCodingSessionIngressEvent` in
 * `desktop/src/features/coding-sessions/lib/codingSessionTrustedIngress.ts`.
 * Three rules are absolute here:
 *
 * 1. Every 442xx fact is signature-verified on this device before it is
 *    decoded into anything a surface can render.
 * 2. Facts from different signers NEVER merge. A stream is keyed by
 *    (channel, target, signer).
 * 3. Authority for a target comes from the `providerAuthorityPubkey` its 44221
 *    create named. When no create is readable the first-seen metadata signer
 *    stands in and the execution is disclosed as `authority unverified` —
 *    a fallback the UI must state, never one it may hide.
 */
import {
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "../../../shared/lib/kinds.ts";
import { hasValidSignature } from "../../../shared/lib/verify.ts";
import type { BeekeeperCodingSessionMetadataV1 } from "./ingressPayloads.ts";
import {
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  type CodingSessionLifecycleReceipt,
  CODING_SESSION_METADATA_TAG_VERSION,
  isCodingSessionTurnReceiptStatus,
  parseBeekeeperCodingSessionMetadata,
  parseCodingSessionLifecycleReceipt,
} from "./ingressPayloads.ts";
import {
  buildCodingSessionTargetKey,
  codingSessionMetadataSemanticKey,
  codingSessionReceiptSemanticKey,
  codingSessionTranscriptSemanticKey,
} from "./keys.ts";
import { canonicalizeProjectionPayload } from "./payload.ts";
import {
  type BeekeeperCodingSessionTranscriptV1,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
  parseBeekeeperCodingSessionTranscript,
} from "./transcriptEnvelope.ts";
import type { ObservedEvent } from "./types.ts";
import { hasTagNamed, normalizePubkey, parseExactTags } from "./wireDecode.ts";

/** What one observed event turned out to be. */
export type TrustedIngressClassification =
  | { kind: "irrelevant" }
  | { kind: "malformed" }
  | { kind: "invalid-signature" }
  | {
      kind: "metadata";
      channelId: string;
      signerPubkey: string;
      targetKey: string;
      metadata: Readonly<BeekeeperCodingSessionMetadataV1>;
      canonicalPayload: string;
    }
  | {
      kind: "receipt";
      channelId: string;
      signerPubkey: string;
      receipt: Readonly<CodingSessionLifecycleReceipt>;
      canonicalPayload: string;
    }
  | {
      kind: "transcript";
      channelId: string;
      signerPubkey: string;
      targetKey: string;
      transcript: Readonly<BeekeeperCodingSessionTranscriptV1>;
      canonicalPayload: string;
    };

/**
 * Classify one observed 44223/44224/44225.
 *
 * `allowedChannelIds` is the channel scope the caller subscribed to; an event
 * whose `h` tag is outside it is malformed for this read, not merely
 * uninteresting — the relay should never have delivered it.
 */
export function classifyCodingSessionEvent(
  event: ObservedEvent,
  allowedChannelIds: ReadonlySet<string>,
): TrustedIngressClassification {
  const supportedKind =
    event.kind === KIND_CODING_SESSION_METADATA ||
    event.kind === KIND_CODING_SESSION_LIFECYCLE_RECEIPT ||
    event.kind === KIND_CODING_SESSION_TRANSCRIPT;
  if (!supportedKind || !Array.isArray(event.tags)) {
    return { kind: "irrelevant" };
  }
  const looksLikeReceipt = hasTagNamed(event.tags, "cslr-v");
  const looksLikeMetadata = hasTagNamed(event.tags, "csm-v");
  const looksLikeTranscript = hasTagNamed(event.tags, "cst-v");
  if (
    Number(looksLikeReceipt) +
      Number(looksLikeMetadata) +
      Number(looksLikeTranscript) !==
    1
  ) {
    // A coding-session kind with no version tag, or with two, is a broken
    // producer rather than unrelated traffic — the kind already committed it.
    return { kind: "malformed" };
  }
  if (
    (looksLikeReceipt &&
      event.kind !== KIND_CODING_SESSION_LIFECYCLE_RECEIPT) ||
    (looksLikeMetadata && event.kind !== KIND_CODING_SESSION_METADATA) ||
    (looksLikeTranscript && event.kind !== KIND_CODING_SESSION_TRANSCRIPT)
  ) {
    return { kind: "malformed" };
  }
  const signerPubkey = normalizePubkey(event.pubkey);
  if (!signerPubkey) return { kind: "malformed" };
  if (!hasValidSignature(event)) return { kind: "invalid-signature" };

  if (looksLikeReceipt) {
    return classifyReceipt(event, allowedChannelIds, signerPubkey);
  }
  if (looksLikeTranscript) {
    return classifyTranscript(event, allowedChannelIds, signerPubkey);
  }
  return classifyMetadata(event, allowedChannelIds, signerPubkey);
}

function classifyReceipt(
  event: ObservedEvent,
  allowedChannelIds: ReadonlySet<string>,
  signerPubkey: string,
): TrustedIngressClassification {
  const tags = parseExactTags(event.tags, [
    "h",
    "cslr-v",
    "csl-command",
    "csl-key",
  ]);
  if (
    !tags ||
    !allowedChannelIds.has(tags[0]) ||
    tags[1] !== CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION
  ) {
    return { kind: "malformed" };
  }
  const receipt = parseCodingSessionLifecycleReceipt(event.content);
  if (
    !receipt ||
    tags[2] !== receipt.commandId ||
    // Per-stage for a turn receipt, single-field for a lifecycle one: the tag
    // must match the key the publish queue actually fenced on, or the producer
    // and this reader disagree about what is a duplicate.
    tags[3] !==
      codingSessionReceiptSemanticKey(
        receipt.commandId,
        receipt.status,
        isCodingSessionTurnReceiptStatus(receipt.status),
      )
  ) {
    return { kind: "malformed" };
  }
  return {
    kind: "receipt",
    channelId: tags[0],
    signerPubkey,
    receipt,
    canonicalPayload: JSON.stringify(receipt),
  };
}

function classifyTranscript(
  event: ObservedEvent,
  allowedChannelIds: ReadonlySet<string>,
  signerPubkey: string,
): TrustedIngressClassification {
  const tags = parseExactTags(event.tags, [
    "h",
    "cst-v",
    "cs-target",
    "cst-seq",
    "cst-key",
  ]);
  if (
    !tags ||
    !allowedChannelIds.has(tags[0]) ||
    tags[1] !== CODING_SESSION_TRANSCRIPT_TAG_VERSION
  ) {
    return { kind: "malformed" };
  }
  const transcript = parseBeekeeperCodingSessionTranscript(event.content);
  if (!transcript) return { kind: "malformed" };
  const targetKey = buildCodingSessionTargetKey(transcript.session);
  if (
    tags[2] !== targetKey ||
    tags[3] !== String(transcript.eventSeq) ||
    tags[4] !==
      codingSessionTranscriptSemanticKey(
        transcript.session,
        transcript.eventSeq,
      )
  ) {
    return { kind: "malformed" };
  }
  const canonicalPayload = canonicalizeProjectionPayload(transcript);
  if (canonicalPayload === null) return { kind: "malformed" };
  return {
    kind: "transcript",
    channelId: tags[0],
    signerPubkey,
    targetKey,
    transcript,
    canonicalPayload,
  };
}

function classifyMetadata(
  event: ObservedEvent,
  allowedChannelIds: ReadonlySet<string>,
  signerPubkey: string,
): TrustedIngressClassification {
  const tags = parseExactTags(event.tags, [
    "h",
    "csm-v",
    "cs-target",
    "csm-key",
  ]);
  if (
    !tags ||
    !allowedChannelIds.has(tags[0]) ||
    tags[1] !== CODING_SESSION_METADATA_TAG_VERSION
  ) {
    return { kind: "malformed" };
  }
  const metadata = parseBeekeeperCodingSessionMetadata(event.content);
  if (!metadata) return { kind: "malformed" };
  const targetKey = buildCodingSessionTargetKey(metadata.session);
  if (
    tags[2] !== targetKey ||
    tags[3] !== codingSessionMetadataSemanticKey(metadata.session)
  ) {
    return { kind: "malformed" };
  }
  const canonicalPayload = canonicalizeProjectionPayload(metadata);
  if (canonicalPayload === null) return { kind: "malformed" };
  return {
    kind: "metadata",
    channelId: tags[0],
    signerPubkey,
    targetKey,
    metadata,
    canonicalPayload,
  };
}

/**
 * Verify a non-442xx-fact event (44221 create, 44226 genesis, 44229 name,
 * 44252 generated title, 44230 closure, 44227 goal, 24223 lease) before its
 * decoder runs.
 *
 * Exported separately because those decoders are pure and testable without a
 * crypto dependency; the gate belongs to the caller that ingests them.
 */
export function isSignatureVerified(event: ObservedEvent): boolean {
  return hasValidSignature(event);
}
