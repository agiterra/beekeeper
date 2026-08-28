/**
 * `session.hire` — the lead's request that the founder's host seat another
 * agent into the umbrella it is already working in.
 *
 * It rides the same signed envelope as every other 44221 lifecycle command
 * (tags `h` / `csl-v` / `csl-command`, the `buzz-coding-session-lifecycle-
 * command/v1` schema) and carries an action with **exactly** these keys:
 *
 * ```json
 * { "type": "session.hire", "sessionRef": "<uuid>", "genesisRef": "<64hex>",
 *   "role": "<slug>", "providerInstanceRef": null, "model": null,
 *   "brief": "<1..12288 bytes>" }
 * ```
 *
 * Two properties this module exists to hold:
 *
 * 1. **A hire is never half-read.** The classifier accepts the exact key set
 *    or nothing: an unknown key, a missing key, a malformed ref or a role that
 *    is not `[a-z0-9-]{1,64}` all classify `malformed`, so a smuggled payload
 *    can never mean more on this screen than it does at the relay that
 *    validates it with `deny_unknown_fields`.
 * 2. **A relay that has not shipped the action says so out loud.** The relay
 *    rejects an action type it does not know as a malformed lifecycle-command
 *    payload — a sentence about JSON, addressed to nobody. A hire refused that
 *    way is a *deployment* fact, so {@link describeCodingSessionHireFailure}
 *    turns it into the one sentence that names it, and every publisher of a
 *    hire is required to render that rather than the raw rejection.
 */
import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_CODING_SESSION_LIFECYCLE_COMMAND } from "@/shared/constants/kinds";
import { hasValidSignature } from "@/shared/lib/authors";
import {
  isCodingSessionRoleSlug,
  MAX_CODING_SESSION_ROLE_BYTES,
} from "./codingSessionActorSeat";
import {
  CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
  CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION,
  MAX_CODING_SESSION_LIFECYCLE_CONTENT_BYTES,
  MAX_CODING_SESSION_LIFECYCLE_IDENTIFIER_BYTES,
  MAX_CODING_SESSION_LIFECYCLE_REFERENCE_BYTES,
  type CodingSessionLifecycleCommandEventInput,
} from "./codingSessionLifecycleCommand";
import {
  boundedNonempty,
  hasExactKeys,
  isCodingSessionSessionRef,
  isPlainRecord,
  normalizePubkey,
  parseBoundedJson,
  parseExactTags,
} from "./codingSessionWireDecode";

/** The action type, spelled once. */
export const CODING_SESSION_HIRE_ACTION_TYPE = "session.hire" as const;

/**
 * Longest brief a hire may carry, in UTF-8 bytes.
 *
 * The same ceiling the create's `initialTurn` has, because the brief *becomes*
 * that turn: a hire this host would have to truncate to seat is refused before
 * it is signed rather than seated against a brief nobody wrote.
 */
export const MAX_CODING_SESSION_HIRE_BRIEF_BYTES = 12 * 1024;

/** The exact key order the action is serialized in. */
export const CODING_SESSION_HIRE_ACTION_KEYS = [
  "type",
  "sessionRef",
  "genesisRef",
  "role",
  "providerInstanceRef",
  "model",
  "brief",
] as const;

/** The `session.hire` action, exactly as it appears on the wire. */
export type CodingSessionHireAction = {
  type: typeof CODING_SESSION_HIRE_ACTION_TYPE;
  /** Umbrella the new seat joins. */
  sessionRef: string;
  /** The umbrella's authority anchor; the seated create names the same one. */
  genesisRef: string;
  /** Role slug the seat is hired for. */
  role: string;
  /** Runtime the lead asks for, or null to take the host's policy default. */
  providerInstanceRef: string | null;
  /** Model the lead asks for, or null to take the identity's own. */
  model: string | null;
  /** The seat's whole first turn. Non-empty. */
  brief: string;
};

/** A signed hire, once read off the wire. */
export type CodingSessionHireRequest = {
  /** Event id of the 44221 that carried it. */
  eventId: string;
  channelId: string;
  commandId: string;
  /** Whoever signed the request — the lead seat, or an operator. */
  requesterPubkey: string;
  /** Event `created_at`, in seconds. */
  createdAt: number;
  action: CodingSessionHireAction;
};

export type CodingSessionHireClassification =
  | ({ kind: "hire" } & CodingSessionHireRequest)
  | { kind: "irrelevant" }
  | { kind: "malformed" }
  | { kind: "invalid-signature" };

/**
 * Build the unsigned 44221 that asks the founder's host for a seat.
 *
 * Throws — never emits a payload the relay would refuse — so a malformed hire
 * is a caller bug, not a relay round trip.
 */
export function buildCodingSessionHireEvent(input: {
  channelId: string;
  commandId: string;
  sessionRef: string;
  genesisRef: string;
  role: string;
  providerInstanceRef: string | null;
  model: string | null;
  brief: string;
}): CodingSessionLifecycleCommandEventInput {
  validateCodingSessionHireInput(input);
  const content = JSON.stringify({
    schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
    commandId: input.commandId,
    action: {
      type: CODING_SESSION_HIRE_ACTION_TYPE,
      sessionRef: input.sessionRef,
      genesisRef: input.genesisRef,
      role: input.role,
      providerInstanceRef: input.providerInstanceRef,
      model: input.model,
      brief: input.brief,
    },
  });
  if (
    new TextEncoder().encode(content).byteLength >
    MAX_CODING_SESSION_LIFECYCLE_CONTENT_BYTES
  ) {
    throw new Error(
      `coding-session lifecycle command content exceeds ${MAX_CODING_SESSION_LIFECYCLE_CONTENT_BYTES} bytes`,
    );
  }
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

/** Every bound the relay will check, checked here first. */
export function validateCodingSessionHireInput(input: {
  channelId: string;
  commandId: string;
  sessionRef: string;
  genesisRef: string;
  role: string;
  providerInstanceRef: string | null;
  model: string | null;
  brief: string;
}): void {
  if (input.channelId.trim().length === 0) {
    throw new Error("channelId must not be empty");
  }
  if (
    input.commandId.trim().length === 0 ||
    utf8Bytes(input.commandId) > MAX_CODING_SESSION_LIFECYCLE_IDENTIFIER_BYTES
  ) {
    throw new Error(
      `commandId must be non-empty and at most ${MAX_CODING_SESSION_LIFECYCLE_IDENTIFIER_BYTES} bytes`,
    );
  }
  if (!isCodingSessionSessionRef(input.sessionRef)) {
    throw new Error(
      "action.sessionRef must be a canonical lowercase hyphenated UUID",
    );
  }
  if (!/^[0-9a-f]{64}$/.test(input.genesisRef)) {
    throw new Error("action.genesisRef must be a lowercase 64-hex event id");
  }
  if (!isCodingSessionRoleSlug(input.role)) {
    throw new Error(
      `action.role must be [a-z0-9-]+ and at most ${MAX_CODING_SESSION_ROLE_BYTES} bytes`,
    );
  }
  for (const [field, value] of [
    ["action.providerInstanceRef", input.providerInstanceRef],
    ["action.model", input.model],
  ] as const) {
    if (value === null) continue;
    if (
      value.trim().length === 0 ||
      utf8Bytes(value) > MAX_CODING_SESSION_LIFECYCLE_REFERENCE_BYTES
    ) {
      throw new Error(
        `${field} must be non-empty and at most ${MAX_CODING_SESSION_LIFECYCLE_REFERENCE_BYTES} bytes`,
      );
    }
  }
  if (input.brief.trim().length === 0) {
    throw new Error("action.brief must not be empty");
  }
  if (utf8Bytes(input.brief) > MAX_CODING_SESSION_HIRE_BRIEF_BYTES) {
    throw new Error(
      `action.brief exceeds ${MAX_CODING_SESSION_HIRE_BRIEF_BYTES} bytes`,
    );
  }
}

/**
 * Read a signed 44221 as a hire request, or say why it is not one.
 *
 * `irrelevant` is reserved for events that are not lifecycle commands at all,
 * and for lifecycle commands carrying some *other* action — a create is not a
 * malformed hire, and counting it as one would make every ordinary session
 * look like an attack on this store.
 */
export function classifyCodingSessionHireEvent(
  event: RelayEvent,
  allowedChannelIds: ReadonlySet<string>,
): CodingSessionHireClassification {
  if (
    event.kind !== KIND_CODING_SESSION_LIFECYCLE_COMMAND ||
    !Array.isArray(event.tags)
  ) {
    return { kind: "irrelevant" };
  }
  const payload = parseBoundedJson(
    event.content,
    MAX_CODING_SESSION_LIFECYCLE_CONTENT_BYTES,
  );
  // Another action on the same kind is somebody else's business, and it is
  // read before anything is judged so a create never lands in this store's
  // malformed count.
  if (
    !isPlainRecord(payload) ||
    !isPlainRecord(payload.action) ||
    payload.action.type !== CODING_SESSION_HIRE_ACTION_TYPE
  ) {
    return { kind: "irrelevant" };
  }
  const tags = parseExactTags(event.tags, ["h", "csl-v", "csl-command"]);
  if (
    !tags ||
    !allowedChannelIds.has(tags[0]) ||
    tags[1] !== CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION
  ) {
    return { kind: "malformed" };
  }
  const requesterPubkey = normalizePubkey(event.pubkey);
  if (!requesterPubkey) return { kind: "malformed" };
  if (
    !hasExactKeys(payload, ["schema", "commandId", "action"]) ||
    payload.schema !== CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA ||
    !boundedNonempty(
      payload.commandId,
      MAX_CODING_SESSION_LIFECYCLE_IDENTIFIER_BYTES,
    ) ||
    payload.commandId !== tags[2]
  ) {
    return { kind: "malformed" };
  }
  const action = payload.action;
  if (!hasExactKeys(action, [...CODING_SESSION_HIRE_ACTION_KEYS])) {
    return { kind: "malformed" };
  }
  if (
    !isCodingSessionSessionRef(action.sessionRef) ||
    typeof action.genesisRef !== "string" ||
    !/^[0-9a-f]{64}$/.test(action.genesisRef) ||
    !isCodingSessionRoleSlug(action.role) ||
    !isOptionalReference(action.providerInstanceRef) ||
    !isOptionalReference(action.model) ||
    !boundedNonempty(action.brief, MAX_CODING_SESSION_HIRE_BRIEF_BYTES)
  ) {
    return { kind: "malformed" };
  }
  // Last, because it is the expensive one and every cheap refusal above has
  // already run.
  if (!hasValidSignature(event)) return { kind: "invalid-signature" };
  return {
    kind: "hire",
    eventId: event.id,
    channelId: tags[0],
    commandId: payload.commandId,
    requesterPubkey,
    createdAt: event.created_at,
    action: {
      type: CODING_SESSION_HIRE_ACTION_TYPE,
      sessionRef: action.sessionRef,
      genesisRef: action.genesisRef,
      role: action.role,
      providerInstanceRef: action.providerInstanceRef,
      model: action.model,
      brief: action.brief,
    },
  };
}

/**
 * The sentence a relay that predates `session.hire` earns.
 *
 * The relay validates the 44221 action with `deny_unknown_fields`, so a hire
 * sent to a relay that has not shipped it comes back as "malformed
 * coding-session lifecycle command payload" — true, unhelpful, and easy to
 * read as *your request* being wrong. The action is only valid once the relay
 * carrying it is deployed, so the failure is named as what it is.
 */
export const CODING_SESSION_HIRE_UNSUPPORTED_RELAY_MESSAGE =
  "This relay does not accept hire requests yet — it refused the request as " +
  "malformed. The relay has to ship session.hire before a lead can hire a seat.";

/** True for exactly the rejection shape a relay without `session.hire` gives. */
export function isCodingSessionHireUnsupportedRelayFailure(
  error: unknown,
): boolean {
  const message = (
    error instanceof Error ? error.message : String(error ?? "")
  ).toLowerCase();
  return (
    message.includes("lifecycle command") &&
    (message.includes("malformed") || message.includes("invalid"))
  );
}

/**
 * What to show a person when a hire did not go out.
 *
 * Never the raw relay sentence for the one failure that is really a
 * deployment fact — that is the whole wire rule: a hire the relay cannot yet
 * accept fails *visibly*, naming the relay, never silently and never blaming
 * the request.
 */
export function describeCodingSessionHireFailure(error: unknown): string {
  if (isCodingSessionHireUnsupportedRelayFailure(error)) {
    return CODING_SESSION_HIRE_UNSUPPORTED_RELAY_MESSAGE;
  }
  const message = error instanceof Error ? error.message.trim() : "";
  return message.length > 0
    ? `The hire request was not accepted. ${message}`
    : "The hire request was not accepted.";
}

type HirePublisher = {
  publishEvent: (
    event: RelayEvent,
    timeoutMessage: string,
    sendErrorMessage: string,
  ) => Promise<RelayEvent>;
};

type HireSigner = (
  input: CodingSessionLifecycleCommandEventInput,
) => Promise<RelayEvent>;

/**
 * Sign and publish a hire request.
 *
 * Rejects with {@link describeCodingSessionHireFailure}'s sentence already
 * applied, so no caller can accidentally surface the raw
 * "malformed … payload" and leave a person believing their brief was wrong.
 */
export async function publishCodingSessionHire(
  input: Parameters<typeof buildCodingSessionHireEvent>[0],
  dependencies: { publisher?: HirePublisher; signer?: HireSigner } = {},
): Promise<{ eventId: string; commandId: string }> {
  const eventInput = buildCodingSessionHireEvent(input);
  const publisher = dependencies.publisher ?? relayClient;
  const signer = dependencies.signer ?? signRelayEvent;
  const event = await signer(eventInput);
  try {
    const accepted = await publisher.publishEvent(
      event,
      "Timed out while sending the hire request.",
      "Failed to send the hire request.",
    );
    return { eventId: accepted.id, commandId: input.commandId };
  } catch (error) {
    throw new Error(describeCodingSessionHireFailure(error));
  }
}

function isOptionalReference(value: unknown): value is string | null {
  return (
    value === null ||
    boundedNonempty(value, MAX_CODING_SESSION_LIFECYCLE_REFERENCE_BYTES)
  );
}

function utf8Bytes(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}
