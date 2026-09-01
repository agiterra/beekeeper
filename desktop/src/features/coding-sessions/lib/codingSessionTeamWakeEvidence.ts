/**
 * What Desktop is allowed to believe about a team wake it did not send.
 *
 * The duplicate-wake defect of 2026-08-31 was not a race: the provider's
 * `turn_queued` receipt was on the relay nine seconds before Desktop's
 * fallback went out, and Desktop simply had no code that read it. Its only
 * suppression signal was the lead's user-prompt echo, which does not exist
 * until a turn *starts* — so any wake queued behind a long lead turn fell
 * between the provider's bar and Desktop's.
 *
 * This module is the missing evidence plane. Two strict decoders and one
 * bounded index answer three questions about one exact lead generation:
 * which 44220 commands carry this operation's pointer, how far each got, and
 * which of them the runner refused as a duplicate.
 *
 * Trust rules, and they are the whole point:
 *
 * - A **command** is evidence only when its signature verifies, it is scoped
 *   to this channel, and its *payload* target — not merely its `cs-target`
 *   tag — is the exact lead generation.
 * - A **receipt** is evidence only when it is signed by that lead execution's
 *   own `providerAuthorityPubkey` and its `session` target is that same exact
 *   generation. Nobody else's signature settles anything (I15).
 * - Pointer matching is byte equality against `codingSessionTeamWakeText`.
 *   Both producers deliberately emit the same bytes, and the runner's own
 *   canonicalisation is its own fence; normalising here would only invent
 *   matches neither producer made (I2).
 */
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_COMMAND,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
} from "@/shared/constants/kinds";
import { hasValidSignature } from "@/shared/lib/authors";
import {
  buildCodingSessionTargetKey,
  CODING_SESSION_COMMAND_SCHEMA,
  MAX_CODING_SESSION_IDENTIFIER_BYTES,
  MAX_CODING_SESSION_TEXT_BYTES,
} from "./codingSessionCommand";
import {
  isCodingSessionTurnReceipt,
  parseCodingSessionLifecycleReceipt,
  type CodingSessionTurnReceipt,
} from "./codingSessionIngressPayloads";
import { CODING_SESSION_DUPLICATE_OPERATION_CODE } from "./codingSessionMissionContracts";
import {
  CodingSessionTurnReceiptIndex,
  type CodingSessionTurnFailure,
  type CodingSessionTurnProgress,
} from "./codingSessionTurnReceiptIndex";
import {
  boundedNonempty,
  decodeTarget,
  hasExactKeys,
  hasRequiredAndOptionalKeys,
  isPlainRecord,
  parseBoundedJson,
} from "./codingSessionWireDecode";

/** Largest 44220 payload this decoder will parse before rejecting outright. */
const MAX_COMMAND_CONTENT_BYTES = 16 * 1024;
/** Default retained verified commands for one lead target. */
export const CODING_SESSION_TEAM_WAKE_COMMAND_LIMIT = 512;
/** Default retained verified turn receipts for one lead target. */
export const CODING_SESSION_TEAM_WAKE_RECEIPT_LIMIT = 2_048;

/** The scope every decode and every index answer is bound to. */
export type CodingSessionTeamWakeEvidenceContext = {
  channelId: string;
  leadTargetKey: string;
  /** The lead execution's own provider authority. Only this key signs receipts. */
  providerAuthorityPubkey: string;
};

/** A verified 44220 `thread.turn.start` addressed to the exact lead target. */
export type CodingSessionTeamWakeCommandEvidence = {
  eventId: string;
  commandId: string;
  signerPubkey: string;
  targetKey: string;
  /** The command's exact turn text — the pointer, byte for byte. */
  text: string;
  /** Signed publication second. Display and ordering only; never dedupe (I4). */
  createdAt: number;
};

/** A verified 44224 turn receipt from the lead's own provider authority. */
export type CodingSessionTeamWakeReceiptEvidence = {
  eventId: string;
  commandId: string;
  signerPubkey: string;
  createdAt: number;
  /** The exact signed content, used to detect a self-contradicting authority. */
  canonicalPayload: string;
  receipt: Readonly<CodingSessionTurnReceipt>;
};

function tagValue(event: RelayEvent, name: string): string | null {
  for (const tag of event.tags) {
    if (tag.length >= 2 && tag[0] === name) return tag[1];
  }
  return null;
}

function isTurnStartAction(value: unknown): value is { text: string } {
  return (
    isPlainRecord(value) &&
    value.type === "thread.turn.start" &&
    hasRequiredAndOptionalKeys(
      value,
      ["type", "text"],
      ["attachments", "deliver"],
    ) &&
    boundedNonempty(value.text, MAX_CODING_SESSION_TEXT_BYTES)
  );
}

/**
 * Decode one 44220 as a wake command for this exact lead generation, or
 * `null`.
 *
 * The signature check is last because it is the expensive one and every cheap
 * scope check above it rejects the overwhelming majority of a channel's
 * traffic first.
 */
export function decodeVerifiedTeamWakeCommand(
  event: RelayEvent,
  context: CodingSessionTeamWakeEvidenceContext,
): CodingSessionTeamWakeCommandEvidence | null {
  if (
    event.kind !== KIND_CODING_SESSION_COMMAND ||
    tagValue(event, "h") !== context.channelId ||
    tagValue(event, "cs-target") !== context.leadTargetKey
  ) {
    return null;
  }
  const value = parseBoundedJson(event.content, MAX_COMMAND_CONTENT_BYTES);
  if (
    !isPlainRecord(value) ||
    !hasExactKeys(value, ["schema", "commandId", "target", "action"]) ||
    value.schema !== CODING_SESSION_COMMAND_SCHEMA ||
    !boundedNonempty(value.commandId, MAX_CODING_SESSION_IDENTIFIER_BYTES) ||
    !isTurnStartAction(value.action)
  ) {
    return null;
  }
  const target = decodeTarget(value.target);
  if (
    !target ||
    buildCodingSessionTargetKey(target) !== context.leadTargetKey
  ) {
    return null;
  }
  if (!hasValidSignature(event)) return null;
  return {
    eventId: event.id,
    commandId: value.commandId,
    signerPubkey: event.pubkey.toLowerCase(),
    targetKey: context.leadTargetKey,
    text: value.action.text,
    createdAt: event.created_at,
  };
}

/**
 * Decode one 44224 as a turn receipt this lead's provider actually signed, or
 * `null`.
 *
 * Both halves of the trust rule are load-bearing. The signer check keeps a
 * stranger from claiming an operation was delivered; the `session` check keeps
 * a genuine receipt about the *previous* generation from settling this one.
 */
export function decodeVerifiedTeamWakeReceipt(
  event: RelayEvent,
  context: CodingSessionTeamWakeEvidenceContext,
): CodingSessionTeamWakeReceiptEvidence | null {
  if (
    event.kind !== KIND_CODING_SESSION_LIFECYCLE_RECEIPT ||
    tagValue(event, "h") !== context.channelId ||
    event.pubkey.toLowerCase() !== context.providerAuthorityPubkey.toLowerCase()
  ) {
    return null;
  }
  const receipt = parseCodingSessionLifecycleReceipt(event.content);
  if (
    !receipt ||
    !isCodingSessionTurnReceipt(receipt) ||
    receipt.session === null ||
    buildCodingSessionTargetKey(receipt.session) !== context.leadTargetKey
  ) {
    return null;
  }
  if (!hasValidSignature(event)) return null;
  return {
    eventId: event.id,
    commandId: receipt.commandId,
    signerPubkey: event.pubkey.toLowerCase(),
    createdAt: event.created_at,
    canonicalPayload: event.content,
    receipt,
  };
}

/** Optional bounds, so a test can exercise overflow without 512 fixtures. */
export type CodingSessionTeamWakeEvidenceBounds = {
  maxCommands?: number;
  maxReceipts?: number;
};

/**
 * Bounded, scope-local index of the commands and receipts that address one
 * lead generation.
 *
 * It owns no module-level state, so a community switch or a generation rebump
 * simply builds a new one. `overflowed` is public because a saturated index is
 * incomplete evidence, and incomplete evidence must read as `unknown` rather
 * than as absence.
 */
export class CodingSessionTeamWakeEvidenceIndex {
  readonly #context: CodingSessionTeamWakeEvidenceContext;
  readonly #maxCommands: number;
  readonly #maxReceipts: number;
  readonly #commands = new Map<string, CodingSessionTeamWakeCommandEvidence>();
  readonly #byText = new Map<string, CodingSessionTeamWakeCommandEvidence[]>();
  readonly #receipts = new CodingSessionTurnReceiptIndex();
  readonly #receiptEventIds = new Set<string>();
  #overflowed = false;
  #revision = 0;

  constructor(
    context: CodingSessionTeamWakeEvidenceContext,
    bounds: CodingSessionTeamWakeEvidenceBounds = {},
  ) {
    this.#context = context;
    this.#maxCommands =
      bounds.maxCommands ?? CODING_SESSION_TEAM_WAKE_COMMAND_LIMIT;
    this.#maxReceipts =
      bounds.maxReceipts ?? CODING_SESSION_TEAM_WAKE_RECEIPT_LIMIT;
  }

  /** True once a bound was hit; the index is then incomplete and says so. */
  get overflowed(): boolean {
    return this.#overflowed;
  }

  /** Monotonic counter bumped by every accepted event, for render identity. */
  get revision(): number {
    return this.#revision;
  }

  /** Verify and retain any of these events that address the lead target. */
  ingest(events: readonly RelayEvent[]): boolean {
    let changed = false;
    for (const event of events) {
      if (this.#ingestOne(event)) changed = true;
    }
    if (changed) this.#revision += 1;
    return changed;
  }

  /** Every verified command whose turn text is exactly this pointer. */
  commandsFor(
    pointerText: string,
  ): readonly CodingSessionTeamWakeCommandEvidence[] {
    return this.#byText.get(pointerText) ?? [];
  }

  /** How far the provider says this command got, or `null` if it said nothing. */
  progressFor(commandId: string): CodingSessionTurnProgress | null {
    return this.#receipts.resolveProgress(
      this.#context.channelId,
      commandId,
      this.#context.providerAuthorityPubkey,
    );
  }

  /** The dropped/refused outcome for this command, or `null` if it still has a run. */
  failureFor(commandId: string): CodingSessionTurnFailure | null {
    return this.#receipts.resolveFailure(
      this.#context.channelId,
      commandId,
      this.#context.providerAuthorityPubkey,
    );
  }

  /**
   * Commands the runner answered `DUPLICATE_OPERATION` for this pointer.
   *
   * These spent no turn and are settlement, never failure (I13) — the caller
   * discloses them beside whichever command actually owns the operation.
   */
  duplicateRefusedFor(pointerText: string): readonly string[] {
    return this.commandsFor(pointerText)
      .filter(
        (command) =>
          this.failureFor(command.commandId)?.code ===
          CODING_SESSION_DUPLICATE_OPERATION_CODE,
      )
      .map((command) => command.commandId);
  }

  #ingestOne(event: RelayEvent): boolean {
    if (event.kind === KIND_CODING_SESSION_COMMAND) {
      if (this.#commands.has(event.id)) return false;
      const command = decodeVerifiedTeamWakeCommand(event, this.#context);
      if (!command) return false;
      if (this.#commands.size >= this.#maxCommands) {
        this.#overflowed = true;
        return false;
      }
      this.#commands.set(event.id, command);
      const bucket = this.#byText.get(command.text) ?? [];
      bucket.push(command);
      bucket.sort(
        (left, right) =>
          left.createdAt - right.createdAt ||
          left.commandId.localeCompare(right.commandId),
      );
      this.#byText.set(command.text, bucket);
      return true;
    }
    if (event.kind !== KIND_CODING_SESSION_LIFECYCLE_RECEIPT) return false;
    if (this.#receiptEventIds.has(event.id)) return false;
    const receipt = decodeVerifiedTeamWakeReceipt(event, this.#context);
    if (!receipt) return false;
    if (this.#receiptEventIds.size >= this.#maxReceipts) {
      this.#overflowed = true;
      return false;
    }
    this.#receiptEventIds.add(event.id);
    this.#receipts.record(
      this.#context.channelId,
      receipt.commandId,
      receipt.receipt.status,
      {
        eventId: receipt.eventId,
        createdAt: receipt.createdAt,
        signerPubkey: receipt.signerPubkey,
        canonicalPayload: receipt.canonicalPayload,
        value: receipt.receipt,
      },
    );
    return true;
  }
}
