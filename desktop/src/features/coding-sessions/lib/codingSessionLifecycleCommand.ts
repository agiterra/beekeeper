import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_CODING_SESSION_LIFECYCLE_COMMAND } from "@/shared/constants/kinds";
import type { CodingSessionCommandTarget } from "./codingSessionCommand";
import { isCodingSessionSessionRef } from "./codingSessionWireDecode";

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
 *
 * `sessionRef` is the umbrella session identity: a client-minted lowercase
 * UUID claimed on the create and shared by every execution of the same
 * umbrella session. Unlike `projectRef` it joined the schema after v1 events
 * were already signed, so the key itself is optional on the type: new
 * producers always write it (explicit `null` means "no umbrella claimed"),
 * while an absent key is only ever the historical 8-key form re-serialized
 * byte-for-byte (durable-create replay).
 */
export type CodingSessionCreateAction = {
  type: "session.create";
  projectRef: string | null;
  repoRef: string | null;
  sessionRef?: string | null;
  providerInstanceRef: string;
  providerAuthorityPubkey: string;
  model: string | null;
  title: string | null;
  initialTurn: string | null;
};

export type CodingSessionResumeAction = {
  type: "session.resume";
  session: CodingSessionCommandTarget;
  providerAuthorityPubkey: string;
};

export type CodingSessionStopAction = {
  type: "session.stop";
  session: CodingSessionCommandTarget;
  providerAuthorityPubkey: string;
};

export type CodingSessionLifecycleAction =
  | CodingSessionCreateAction
  | CodingSessionResumeAction
  | CodingSessionStopAction;

export type CodingSessionLifecycleCommandPayload = {
  schema: typeof CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA;
  commandId: string;
  action: CodingSessionLifecycleAction;
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
  /**
   * New creates always carry the key — a freshly minted UUID
   * ({@link createCodingSessionSessionRef}) or an explicit `null`. An absent
   * key reproduces the historical 8-key action exactly, which durable-create
   * replay of pre-`sessionRef` transactions depends on.
   */
  sessionRef?: string | null;
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
      // Canonical key order matches the sidecar's 9-key decode form; the
      // spread keeps the historical 8-key form byte-identical when absent.
      // `undefined` counts as absent so a durable transaction round-tripped
      // through JSON (which drops undefined values) rebuilds the same bytes.
      ...(input.sessionRef !== undefined
        ? { sessionRef: input.sessionRef }
        : {}),
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
  sessionRef?: string | null;
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
  if (
    input.sessionRef !== undefined &&
    input.sessionRef !== null &&
    !isCodingSessionSessionRef(input.sessionRef)
  ) {
    throw new Error(
      "action.sessionRef must be a canonical lowercase hyphenated UUID",
    );
  }
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

/** Build an exact-generation request to reattach a disconnected execution. */
export function buildCodingSessionResumeEvent(input: {
  channelId: string;
  commandId: string;
  target: CodingSessionCommandTarget;
  providerAuthorityPubkey: string;
}): CodingSessionLifecycleCommandEventInput {
  return buildCodingSessionTargetLifecycleEvent(input, "session.resume");
}

/** Build an exact-generation durable stop request. */
export function buildCodingSessionStopEvent(input: {
  channelId: string;
  commandId: string;
  target: CodingSessionCommandTarget;
  providerAuthorityPubkey: string;
}): CodingSessionLifecycleCommandEventInput {
  return buildCodingSessionTargetLifecycleEvent(input, "session.stop");
}

function buildCodingSessionTargetLifecycleEvent(
  input: {
    channelId: string;
    commandId: string;
    target: CodingSessionCommandTarget;
    providerAuthorityPubkey: string;
  },
  type: "session.resume" | "session.stop",
): CodingSessionLifecycleCommandEventInput {
  validateTargetLifecycleInput(input);
  const payload: CodingSessionLifecycleCommandPayload = {
    schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
    commandId: input.commandId,
    action: {
      type,
      session: input.target,
      providerAuthorityPubkey: input.providerAuthorityPubkey,
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

function validateTargetLifecycleInput(input: {
  channelId: string;
  commandId: string;
  target: CodingSessionCommandTarget;
  providerAuthorityPubkey: string;
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
  for (const [field, value] of [
    ["action.session.driver", input.target.driver],
    ["action.session.instanceId", input.target.instanceId],
    ["action.session.sessionId", input.target.sessionId],
  ] as const) {
    validateRequired(
      value,
      field,
      MAX_CODING_SESSION_LIFECYCLE_IDENTIFIER_BYTES,
    );
  }
  if (
    !Number.isSafeInteger(input.target.generation) ||
    input.target.generation <= 0
  ) {
    throw new Error(
      "action.session.generation must be a positive safe integer",
    );
  }
  if (!/^[0-9a-f]{64}$/.test(input.providerAuthorityPubkey)) {
    throw new Error(
      "action.providerAuthorityPubkey must be a lowercase 64-hex public key",
    );
  }
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

/** Publish an exact-generation resume request. */
export async function publishCodingSessionResume(
  input: Parameters<typeof buildCodingSessionResumeEvent>[0],
  dependencies: {
    publisher?: LifecyclePublisher;
    signer?: LifecycleSigner;
  } = {},
): Promise<PublishedCodingSessionLifecycleCommand> {
  return publishLifecycleEvent(
    buildCodingSessionResumeEvent(input),
    "Timed out while reconnecting the coding session.",
    "Failed to reconnect the coding session.",
    dependencies,
  );
}

/** Publish an exact-generation durable stop request. */
export async function publishCodingSessionStop(
  input: Parameters<typeof buildCodingSessionStopEvent>[0],
  dependencies: {
    publisher?: LifecyclePublisher;
    signer?: LifecycleSigner;
  } = {},
): Promise<PublishedCodingSessionLifecycleCommand> {
  return publishLifecycleEvent(
    buildCodingSessionStopEvent(input),
    "Timed out while stopping the coding session.",
    "Failed to stop the coding session.",
    dependencies,
  );
}

async function publishLifecycleEvent(
  input: CodingSessionLifecycleCommandEventInput,
  timeoutMessage: string,
  sendErrorMessage: string,
  dependencies: {
    publisher?: LifecyclePublisher;
    signer?: LifecycleSigner;
  },
): Promise<PublishedCodingSessionLifecycleCommand> {
  const publisher = dependencies.publisher ?? relayClient;
  const signer = dependencies.signer ?? signRelayEvent;
  const event = await signer(input);
  const accepted = await publisher.publishEvent(
    event,
    timeoutMessage,
    sendErrorMessage,
  );
  return { eventId: accepted.id, kind: accepted.kind };
}

export function createCodingSessionLifecycleCommandId(): string {
  return `csl-${crypto.randomUUID()}`;
}

/**
 * Mint the umbrella `sessionRef` a create claims.
 *
 * Every new create mints one — a single-execution session is an umbrella of
 * one, so a later create carrying the same ref joins it as a new execution
 * with no migration step. Deliberately distinct from every provider-runtime
 * identifier: provider session ids live inside `cs-target`.
 */
export function createCodingSessionSessionRef(): string {
  return crypto.randomUUID().toLowerCase();
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
