import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_CODING_SESSION_LIFECYCLE_COMMAND } from "@/shared/constants/kinds";

export const CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA =
  "buzz-coding-session-lifecycle-command/v1" as const;
export const CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION = "csl1-1" as const;

export const MAX_CODING_SESSION_LIFECYCLE_CONTENT_BYTES = 16 * 1024;
export const MAX_CODING_SESSION_LIFECYCLE_IDENTIFIER_BYTES = 256;
export const MAX_CODING_SESSION_LIFECYCLE_REFERENCE_BYTES = 2 * 1024;
export const MAX_CODING_SESSION_LIFECYCLE_INITIAL_TURN_BYTES = 12 * 1024;

/**
 * `projectRef` is nullable — an explicit `null` is a standalone session that
 * belongs to no project. The key is always serialized: a missing key is
 * malformed, not a standalone session.
 */
export type CodingSessionCreateAction = {
  type: "session.create";
  projectRef: string | null;
  repoRef: string | null;
  providerInstanceRef: string;
  providerAuthorityPubkey: string;
  model: string | null;
  title: string | null;
  initialTurn: string | null;
};

export type CodingSessionLifecycleCommandPayload = {
  schema: typeof CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA;
  commandId: string;
  action: CodingSessionCreateAction;
};

export type CodingSessionLifecycleCommandEventInput = {
  content: string;
  kind: number;
  tags: string[][];
};

export type PublishedCodingSessionLifecycleCommand = {
  eventId: string;
  kind: number;
};

type LifecyclePublisher = {
  publishEvent: (
    event: RelayEvent,
    timeoutMessage: string,
    sendErrorMessage: string,
  ) => Promise<RelayEvent>;
};

type LifecycleSigner = (
  input: CodingSessionLifecycleCommandEventInput,
) => Promise<RelayEvent>;

export function buildCodingSessionCreateEvent(input: {
  channelId: string;
  commandId: string;
  projectRef: string | null;
  repoRef: string | null;
  providerInstanceRef: string;
  providerAuthorityPubkey: string;
  model: string | null;
  title: string | null;
  initialTurn: string | null;
}): CodingSessionLifecycleCommandEventInput {
  validateCodingSessionCreateInput(input);
  const payload: CodingSessionLifecycleCommandPayload = {
    schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
    commandId: input.commandId,
    action: {
      type: "session.create",
      projectRef: input.projectRef,
      repoRef: input.repoRef,
      providerInstanceRef: input.providerInstanceRef,
      providerAuthorityPubkey: input.providerAuthorityPubkey,
      model: input.model,
      title: input.title,
      initialTurn: input.initialTurn,
    },
  };
  const content = JSON.stringify(payload);
  validateUtf8Limit(
    content,
    "coding-session lifecycle command content",
    MAX_CODING_SESSION_LIFECYCLE_CONTENT_BYTES,
  );
  return {
    kind: KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    content,
    tags: [
      ["h", input.channelId],
      ["csl-v", CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION],
      ["csl-command", input.commandId],
    ],
  };
}

export function validateCodingSessionCreateInput(input: {
  channelId: string;
  commandId: string;
  projectRef: string | null;
  repoRef: string | null;
  providerInstanceRef: string;
  providerAuthorityPubkey: string;
  model: string | null;
  title: string | null;
  initialTurn: string | null;
}): void {
  validateRequired(
    input.channelId,
    "channelId",
    MAX_CODING_SESSION_LIFECYCLE_REFERENCE_BYTES,
  );
  validateRequired(
    input.commandId,
    "commandId",
    MAX_CODING_SESSION_LIFECYCLE_IDENTIFIER_BYTES,
  );
  validateOptional(
    input.projectRef,
    "action.projectRef",
    MAX_CODING_SESSION_LIFECYCLE_REFERENCE_BYTES,
  );
  validateOptional(
    input.repoRef,
    "action.repoRef",
    MAX_CODING_SESSION_LIFECYCLE_REFERENCE_BYTES,
  );
  validateRequired(
    input.providerInstanceRef,
    "action.providerInstanceRef",
    MAX_CODING_SESSION_LIFECYCLE_REFERENCE_BYTES,
  );
  if (!/^[0-9a-f]{64}$/.test(input.providerAuthorityPubkey)) {
    throw new Error(
      "action.providerAuthorityPubkey must be a lowercase 64-hex public key",
    );
  }
  validateOptional(
    input.model,
    "action.model",
    MAX_CODING_SESSION_LIFECYCLE_REFERENCE_BYTES,
  );
  validateOptional(
    input.title,
    "action.title",
    MAX_CODING_SESSION_LIFECYCLE_REFERENCE_BYTES,
  );
  validateOptional(
    input.initialTurn,
    "action.initialTurn",
    MAX_CODING_SESSION_LIFECYCLE_INITIAL_TURN_BYTES,
  );
}

/**
 * Publish a native 44221 create. The fork owns its relay, so there is no
 * compatibility transport: any rejection fails the create.
 */
export async function publishCodingSessionCreate(
  input: Parameters<typeof buildCodingSessionCreateEvent>[0],
  dependencies: {
    publisher?: LifecyclePublisher;
    signer?: LifecycleSigner;
  } = {},
): Promise<PublishedCodingSessionLifecycleCommand> {
  const eventInput = buildCodingSessionCreateEvent(input);
  const publisher = dependencies.publisher ?? relayClient;
  const signer = dependencies.signer ?? signRelayEvent;
  const event = await signer(eventInput);
  const accepted = await publisher.publishEvent(
    event,
    "Timed out while creating the coding session.",
    "Failed to create the coding session.",
  );
  return { eventId: accepted.id, kind: accepted.kind };
}

export function createCodingSessionLifecycleCommandId(): string {
  return `csl-${crypto.randomUUID()}`;
}

function validateRequired(
  value: string,
  field: string,
  maxBytes: number,
): void {
  if (value.trim().length === 0) {
    throw new Error(`${field} must not be empty`);
  }
  validateUtf8Limit(value, field, maxBytes);
}

function validateOptional(
  value: string | null,
  field: string,
  maxBytes: number,
): void {
  if (value === null) return;
  validateRequired(value, field, maxBytes);
}

function validateUtf8Limit(
  value: string,
  field: string,
  maxBytes: number,
): void {
  const byteLength = new TextEncoder().encode(value).byteLength;
  if (byteLength > maxBytes) {
    throw new Error(`${field} exceeds ${maxBytes} bytes`);
  }
}
