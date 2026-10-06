import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_CODING_SESSION_LIFECYCLE_COMMAND } from "@/shared/constants/kinds";
import {
  isCodingSessionRoleSlug,
  MAX_CODING_SESSION_ROLE_BYTES,
} from "./codingSessionActorSeat";
import type { CodingSessionCommandTarget } from "./codingSessionCommand";
import {
  isStrictCodingSessionRoutingRecord,
  type CodingSessionRoutingRecord,
} from "./codingSessionRouting";
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
 *
 * `genesisRef` is the immutable authority anchor. It is present only in the
 * 10-key form and requires a non-null `sessionRef`; omitting it preserves the
 * historical 8-key form or the interim 9-key umbrella form exactly.
 *
 * `actor` and `role` are the agent seat (NIP-CSL): the managed agent's public
 * key whose identity the provider injects into this execution, and the role
 * slug that labels it. They are present together or not at all — one without
 * the other is malformed and refused before signing — and their absence
 * leaves every earlier form byte-for-byte unchanged. The secret half of the
 * actor's identity is staged host-locally and never appears here.
 */
export type CodingSessionCreateAction = {
  type: "session.create";
  projectRef: string | null;
  repoRef: string | null;
  sessionRef?: string | null;
  genesisRef?: string;
  actor?: string;
  role?: string;
  providerInstanceRef: string;
  providerAuthorityPubkey: string;
  model: string | null;
  title: string | null;
  initialTurn: string | null;
  /**
   * Event id of the kind:44221 `session.hire` this create answers.
   *
   * Additive and optional exactly like `sessionRef`, `genesisRef` and the seat
   * pair before it: an absent key is the pre-amendment form and stays valid
   * forever, and an explicit `null` is refused rather than read as "absent"
   * ({@link CODING_SESSION_HIRE_REF_ON_HIRE_REFUSAL}'s sibling rule). It
   * closes the attribution loop from the create's end — a seated create naming
   * no hire is a seat nobody can attribute to a request.
   */
  hireRef?: string;
};

/**
 * Verbatim refusal when a create carries the hire's `requestedBy`.
 *
 * Frozen: `crates/beekeeper-core/testdata/coding_session_hire_requester/vectors.json`
 * quotes this sentence as the one a TypeScript decoder must also produce, and
 * a test in this feature asserts both sides still say it. "action has missing
 * or unsupported fields" is true, unactionable, and is what the Rust decoder
 * said until REVIEW-B1 F1.
 */
export const CODING_SESSION_REQUESTED_BY_ON_CREATE_REFUSAL =
  "coding-session lifecycle command action.requestedBy is a hire field and " +
  "does not belong on a create: requestedBy names the seat that ran `bee " +
  "sessions hire`; a create names the hire it answers with hireRef";

/** Verbatim refusal when a hire carries the create's `hireRef`. */
export const CODING_SESSION_HIRE_REF_ON_HIRE_REFUSAL =
  "coding-session lifecycle command action.hireRef is a create field and " +
  "does not belong on a hire: a hire cannot answer itself; the founder's host " +
  "writes hireRef on the create it publishes in reply";

/** True for a lowercase 64-hex event id or pubkey — never an uppercase copy. */
export function isCodingSessionLifecycleHex64(value: unknown): value is string {
  return typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
}

/**
 * The refusal an action earns for carrying the *other* action's additive key,
 * or null when neither key is crossed.
 *
 * The two keys never cross, and the refusal names the offending key and says
 * where it belongs, because a reader who is told only that the key set is
 * wrong fixes it by guessing.
 */
export function describeCrossedCodingSessionLifecycleKey(
  action: unknown,
): string | null {
  if (typeof action !== "object" || action === null) return null;
  const type = (action as { type?: unknown }).type;
  if (type === "session.create" && Object.hasOwn(action, "requestedBy")) {
    return CODING_SESSION_REQUESTED_BY_ON_CREATE_REFUSAL;
  }
  if (type === "session.hire" && Object.hasOwn(action, "hireRef")) {
    return CODING_SESSION_HIRE_REF_ON_HIRE_REFUSAL;
  }
  return null;
}

export type CodingSessionResumeAction = {
  type: "session.resume";
  session: CodingSessionCommandTarget;
  providerAuthorityPubkey: string;
};

export type CodingSessionRestartAction = {
  type: "session.restart";
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
  | CodingSessionRestartAction
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
  /** Event id of the genesis this create explicitly names (10-key form). */
  genesisRef?: string;
  /** Lowercase 64-hex pubkey of the managed agent seated on this execution. */
  actor?: string;
  /** Role slug for the seat. Required exactly when `actor` is present. */
  role?: string;
  providerInstanceRef: string;
  providerAuthorityPubkey: string;
  model: string | null;
  title: string | null;
  initialTurn: string | null;
  /**
   * The 44221 hire this create answers, when it answers one (2026-09-01).
   *
   * Trailing and present-or-absent, exactly like `routing` after it: a create
   * nobody hired is byte-identical to the form every existing consumer already
   * reads.
   */
  hireRef?: string;
  /**
   * The routing decision that chose this seat's execution target, when one
   * did (2026-08-30).
   *
   * Trailing and present-or-absent, exactly like `sessionRef`, `genesisRef`
   * and the seat pair before it: a create nobody routed is byte-identical to
   * the form every existing consumer already reads. It is published so the
   * decision is *on the wire* — a seat whose model cannot be explained from
   * the events is a seat running weights nobody can account for.
   */
  routing?: CodingSessionRoutingRecord | null;
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
      ...(input.genesisRef !== undefined
        ? { genesisRef: input.genesisRef }
        : {}),
      // The seat pair is validated as a pair above, so this spread can never
      // emit half of it.
      ...(input.actor !== undefined ? { actor: input.actor } : {}),
      ...(input.role !== undefined ? { role: input.role } : {}),
      providerInstanceRef: input.providerInstanceRef,
      providerAuthorityPubkey: input.providerAuthorityPubkey,
      model: input.model,
      title: input.title,
      initialTurn: input.initialTurn,
      // Additive key order after the historical base is `actor, role, hireRef,
      // routing` (REPORT-B1 §3.2). The seat pair is emitted above with the
      // rest of the 10-key form this builder has always written; `hireRef`
      // lands here, before `routing`, so both trailing amendments keep every
      // earlier form byte-identical when absent.
      ...(input.hireRef === undefined ? {} : { hireRef: input.hireRef }),
      ...(input.routing === undefined || input.routing === null
        ? {}
        : { routing: input.routing }),
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
  genesisRef?: string;
  actor?: string;
  role?: string;
  providerInstanceRef: string;
  providerAuthorityPubkey: string;
  model: string | null;
  title: string | null;
  initialTurn: string | null;
  hireRef?: string;
  routing?: CodingSessionRoutingRecord | null;
}): void {
  if (
    input.routing !== undefined &&
    input.routing !== null &&
    !isStrictCodingSessionRoutingRecord(input.routing)
  ) {
    throw new Error(
      "action.routing must be the closed routing record: class, tier, risk, " +
        "chosen, runnerUp, reason, reviewRequired, reviewReasons, " +
        "challengerSample, override, registryVersion, catalogRevision",
    );
  }
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
  if (input.genesisRef !== undefined) {
    if (input.sessionRef === undefined || input.sessionRef === null) {
      throw new Error("action.genesisRef requires action.sessionRef");
    }
    if (!/^[0-9a-f]{64}$/.test(input.genesisRef)) {
      throw new Error("action.genesisRef must be a lowercase 64-hex event id");
    }
  }
  // Both halves of the seat or neither: a lone `actor` is an unaddressable
  // seat and a lone `role` labels a seat nobody holds. The provider refuses
  // the same pairing with ACTOR_ROLE_PAIR; refusing here means the malformed
  // create is never signed at all.
  if ((input.actor === undefined) !== (input.role === undefined)) {
    throw new Error("action.actor and action.role must be present together");
  }
  if (input.actor !== undefined && !/^[0-9a-f]{64}$/.test(input.actor)) {
    throw new Error("action.actor must be a lowercase 64-hex public key");
  }
  if (input.role !== undefined && !isCodingSessionRoleSlug(input.role)) {
    throw new Error(
      `action.role must be [a-z0-9-]+ and at most ${MAX_CODING_SESSION_ROLE_BYTES} bytes`,
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
  // Lowercase 64-hex, never coerced: pubkeys and event ids are compared
  // byte-for-byte against relay-signed facts, so an uppercase copy is a
  // different string and is rejected rather than folded.
  if (
    input.hireRef !== undefined &&
    !isCodingSessionLifecycleHex64(input.hireRef)
  ) {
    throw new Error("action.hireRef must be a lowercase 64-hex event id");
  }
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

/**
 * Build an exact-generation restart request: detach the live execution and
 * reattach it at once as the next generation with a freshly staged seat
 * (spec § 4.9, "Restart with current definition"). The provider refuses it
 * while a turn is open (`SESSION_BUSY`).
 */
export function buildCodingSessionRestartEvent(input: {
  channelId: string;
  commandId: string;
  target: CodingSessionCommandTarget;
  providerAuthorityPubkey: string;
}): CodingSessionLifecycleCommandEventInput {
  return buildCodingSessionTargetLifecycleEvent(input, "session.restart");
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
  type: "session.resume" | "session.restart" | "session.stop",
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

/** Publish an exact-generation restart request (spec § 4.9). */
export async function publishCodingSessionRestart(
  input: Parameters<typeof buildCodingSessionRestartEvent>[0],
  dependencies: {
    publisher?: LifecyclePublisher;
    signer?: LifecycleSigner;
  } = {},
): Promise<PublishedCodingSessionLifecycleCommand> {
  return publishLifecycleEvent(
    buildCodingSessionRestartEvent(input),
    "Timed out while restarting the coding session.",
    "Failed to restart the coding session.",
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
