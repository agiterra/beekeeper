import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_CODING_SESSION_COMMAND } from "@/shared/constants/kinds";

/** The locked public payload schema for coding-session commands. */
export const CODING_SESSION_COMMAND_SCHEMA = "buzz-coding-session-command/v1";
/** The locked version of the public command tag envelope. */
export const CODING_SESSION_COMMAND_TAG_VERSION = "csc1-1";
/** Maximum UTF-8 byte length for a command or target identifier. */
export const MAX_CODING_SESSION_IDENTIFIER_BYTES = 256;
/** Maximum UTF-8 byte length for a coding-session turn. */
export const MAX_CODING_SESSION_TEXT_BYTES = 12 * 1024;

/** Provider-neutral target for an external coding-session provider adapter. */
export type CodingSessionCommandTarget = {
  driver: string;
  instanceId: string;
  sessionId: string;
  generation: number;
};

/**
 * How the sender asks the provider to deliver this turn into an execution that
 * may already be working. The provider decides what it can honour and says so
 * in its receipts; this is the request, never a promise.
 *
 * - `boundary` — hold the turn in the provider's mailbox and run it when the
 *   current turn settles. This is the contract's default and the only class a
 *   client may assume works.
 * - `steer` — inject into the running turn where the execution's runtime
 *   advertised native steering. Where it did not, the provider publishes
 *   `turn_degraded` (`STEER_UNSUPPORTED`) and treats the turn as `boundary`.
 *   It never cancels the running turn to make room.
 * - `interrupt` — cancel the running turn first, then deliver at the boundary
 *   it creates. Founder authority only; anyone else is refused
 *   (`UNAUTHORIZED_OPERATOR`).
 */
export type CodingSessionTurnDelivery = "boundary" | "steer" | "interrupt";

/** The closed set of delivery classes, in escalation order. */
export const CODING_SESSION_TURN_DELIVERIES = [
  "boundary",
  "steer",
  "interrupt",
] as const;

/** True for exactly the three delivery classes on the wire. */
export function isCodingSessionTurnDelivery(
  value: unknown,
): value is CodingSessionTurnDelivery {
  return (
    typeof value === "string" &&
    (CODING_SESSION_TURN_DELIVERIES as readonly string[]).includes(value)
  );
}

/** Actions supported by the governed coding-session command contract. */
export type CodingSessionCommandAction =
  | {
      type: "thread.turn.start";
      text: string;
      /**
       * Always written explicitly by this client even though the wire
       * contract defaults an absent key to `boundary`: a reader of a signed
       * command should never have to know the default to know what was asked
       * for.
       */
      deliver: CodingSessionTurnDelivery;
    }
  | {
      type: "thread.turn.interrupt";
    };

/** Exact JSON content of a coding-session command. */
export type CodingSessionCommandPayload = {
  schema: typeof CODING_SESSION_COMMAND_SCHEMA;
  commandId: string;
  target: CodingSessionCommandTarget;
  action: CodingSessionCommandAction;
};

/** Signed command event input for the native 44220 publish path. */
export type CodingSessionCommandEventInput = {
  content: string;
  kind: number;
  tags: string[][];
};

/** Result identifying the event the relay actually accepted. */
export type PublishedCodingSessionCommand = {
  eventId: string;
  kind: number;
  /**
   * The command id inside the signed payload, handed back rather than left for
   * the caller to remember. It is the only key a provider receipt for this
   * command carries, so a surface that wants to hear a refusal needs it — and
   * echoing it here keeps that need from tempting anyone to re-mint one.
   */
  commandId: string;
};

type CommandPublisher = {
  publishEvent: (
    event: RelayEvent,
    timeoutMessage: string,
    sendErrorMessage: string,
  ) => Promise<RelayEvent>;
};

type CommandSigner = (
  input: CodingSessionCommandEventInput,
) => Promise<RelayEvent>;

/** Encode target fields as a length-prefixed, unambiguous tag value. */
export function buildCodingSessionTargetKey(
  target: CodingSessionCommandTarget,
): string {
  return encodeStructuredKey(
    "coding-session/v1",
    target.driver,
    target.instanceId,
    target.sessionId,
    String(target.generation),
  );
}

/** Build exact, deterministic content and tags before the OS keystore signs it. */
export function buildCodingSessionCommandEvent(input: {
  channelId: string;
  commandId: string;
  target: CodingSessionCommandTarget;
  text: string;
  deliver: CodingSessionTurnDelivery;
}): CodingSessionCommandEventInput {
  return buildCodingSessionActionEvent({
    channelId: input.channelId,
    commandId: input.commandId,
    target: input.target,
    action: {
      type: "thread.turn.start",
      text: input.text,
      deliver: input.deliver,
    },
  });
}

/** Build an exact generation-fenced interrupt command. */
export function buildCodingSessionInterruptEvent(input: {
  channelId: string;
  commandId: string;
  target: CodingSessionCommandTarget;
}): CodingSessionCommandEventInput {
  return buildCodingSessionActionEvent({
    ...input,
    action: { type: "thread.turn.interrupt" },
  });
}

function buildCodingSessionActionEvent(input: {
  channelId: string;
  commandId: string;
  target: CodingSessionCommandTarget;
  action: CodingSessionCommandAction;
}): CodingSessionCommandEventInput {
  validateCodingSessionCommandInput(input);
  const payload: CodingSessionCommandPayload = {
    schema: CODING_SESSION_COMMAND_SCHEMA,
    commandId: input.commandId,
    target: input.target,
    action: input.action,
  };
  return {
    kind: KIND_CODING_SESSION_COMMAND,
    content: JSON.stringify(payload),
    tags: [
      ["h", input.channelId],
      ["cs-v", CODING_SESSION_COMMAND_TAG_VERSION],
      ["cs-target", buildCodingSessionTargetKey(input.target)],
    ],
  };
}

/** Validate the exact cross-repo command bounds before any signing occurs. */
export function validateCodingSessionCommandInput(input: {
  commandId: string;
  target: CodingSessionCommandTarget;
  action:
    | CodingSessionCommandAction
    | {
        type: "thread.turn.start";
        text?: string;
        deliver?: unknown;
      };
}): void {
  validateBoundedNonemptyUtf8(
    input.commandId,
    "commandId",
    MAX_CODING_SESSION_IDENTIFIER_BYTES,
  );
  validateBoundedNonemptyUtf8(
    input.target.driver,
    "target.driver",
    MAX_CODING_SESSION_IDENTIFIER_BYTES,
  );
  validateBoundedNonemptyUtf8(
    input.target.instanceId,
    "target.instanceId",
    MAX_CODING_SESSION_IDENTIFIER_BYTES,
  );
  validateBoundedNonemptyUtf8(
    input.target.sessionId,
    "target.sessionId",
    MAX_CODING_SESSION_IDENTIFIER_BYTES,
  );
  if (
    !Number.isSafeInteger(input.target.generation) ||
    input.target.generation <= 0
  ) {
    throw new Error("target.generation must be a positive safe integer");
  }
  if (input.action.type === "thread.turn.start") {
    validateBoundedNonemptyUtf8(
      input.action.text ?? "",
      "action.text",
      MAX_CODING_SESSION_TEXT_BYTES,
    );
    // An unrecognised class is refused here rather than sent and defaulted by
    // the provider: a turn the sender asked to interrupt with must never be
    // quietly delivered at a boundary because a typo made it unreadable.
    if (!isCodingSessionTurnDelivery(input.action.deliver)) {
      throw new Error(
        `action.deliver must be one of ${CODING_SESSION_TURN_DELIVERIES.join(", ")}`,
      );
    }
  }
}

/** Publish a command and return the signed event identity accepted by the relay.
 *
 * The fork owns its relay, so 44220 is a native kind with no compatibility
 * transport: every relay rejection — membership, authorization, signature,
 * network, timeout — is a failure of the write.
 */
export async function publishCodingSessionCommand(
  input: Parameters<typeof buildCodingSessionCommandEvent>[0],
  dependencies: {
    publisher?: CommandPublisher;
    signer?: CommandSigner;
  } = {},
): Promise<PublishedCodingSessionCommand> {
  return publishCodingSessionEvent(
    buildCodingSessionCommandEvent(input),
    input.commandId,
    dependencies,
  );
}

/** Publish a signed interrupt for the exact governed catalog target. */
export async function publishCodingSessionInterrupt(
  input: Parameters<typeof buildCodingSessionInterruptEvent>[0],
  dependencies: {
    publisher?: CommandPublisher;
    signer?: CommandSigner;
  } = {},
): Promise<PublishedCodingSessionCommand> {
  return publishCodingSessionEvent(
    buildCodingSessionInterruptEvent(input),
    input.commandId,
    dependencies,
  );
}

async function publishCodingSessionEvent(
  input: CodingSessionCommandEventInput,
  commandId: string,
  dependencies: {
    publisher?: CommandPublisher;
    signer?: CommandSigner;
  },
): Promise<PublishedCodingSessionCommand> {
  const publisher = dependencies.publisher ?? relayClient;
  const signer = dependencies.signer ?? signRelayEvent;
  const event = await signer(input);
  const accepted = await publisher.publishEvent(
    event,
    "Timed out while sending the coding-session command.",
    "Failed to send the coding-session command.",
  );
  return { eventId: accepted.id, kind: accepted.kind, commandId };
}

/**
 * v1 governed targets support both start and interrupt. Keeping this decision
 * at the target-contract boundary avoids provider-specific checks in surfaces.
 */
export function codingSessionTargetSupportsInterrupt(
  target: CodingSessionCommandTarget,
): boolean {
  return (
    target.driver.trim().length > 0 &&
    target.instanceId.trim().length > 0 &&
    target.sessionId.trim().length > 0 &&
    Number.isSafeInteger(target.generation) &&
    target.generation > 0
  );
}

/** Generate a collision-resistant command id without provider-specific meaning. */
export function createCodingSessionCommandId(): string {
  return `csc-${crypto.randomUUID()}`;
}

function encodeStructuredKey(
  domain: string,
  ...fields: readonly string[]
): string {
  const encoder = new TextEncoder();
  return `${domain}|${fields
    .map((field) => `${encoder.encode(field).byteLength}:${field}`)
    .join("")}`;
}

function validateBoundedNonemptyUtf8(
  value: string,
  field: string,
  maxBytes: number,
): void {
  if (value.trim().length === 0) {
    throw new Error(`${field} must not be empty`);
  }
  const byteLength = new TextEncoder().encode(value).byteLength;
  if (byteLength > maxBytes) {
    throw new Error(`${field} exceeds ${maxBytes} bytes`);
  }
}
