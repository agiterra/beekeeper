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
 *   "brief": "<1..12272 bytes>", "routing": null }
 * ```
 *
 * `routing` is the 2026-08-30 amendment and travels present-or-absent, never
 * as a key the old seven-key form has to grow: a hire that predates the
 * router is still exactly seven keys and is still honoured. When it *is*
 * present it carries what the lead knows — the class, the risk, and any extra
 * trait minimums — and nothing it does not: the lead never names a model, so
 * the router fills `chosen`, `runnerUp`, `reason` and the rest on the seat's
 * create. See {@link CodingSessionHireRoutingRequest}.
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
  readCodingSessionHireRoutingRequest,
  type CodingSessionHireRoutingRequest,
} from "./codingSessionHireRouting";
import { CODING_SESSION_HIRE_BRIEF_PREFIX } from "./codingSessionHireSeat";
import {
  CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
  CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION,
  describeCrossedCodingSessionLifecycleKey,
  isCodingSessionLifecycleHex64,
  MAX_CODING_SESSION_LIFECYCLE_CONTENT_BYTES,
  MAX_CODING_SESSION_LIFECYCLE_IDENTIFIER_BYTES,
  MAX_CODING_SESSION_LIFECYCLE_INITIAL_TURN_BYTES,
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
 * *Not* the create's `initialTurn` ceiling, but that ceiling minus the host's
 * {@link CODING_SESSION_HIRE_BRIEF_PREFIX} — because the brief becomes that
 * turn *behind the prefix* (`codingSessionHireSeat.ts:130`), so a brief that
 * fills the wider ceiling produces an initial turn 16 bytes over it. Derived
 * from both constants rather than written down, and mirrored in Rust by
 * `MAX_LIFECYCLE_HIRE_BRIEF_BYTES`
 * (`crates/buzz-core/src/coding_session_lifecycle_command.rs:80`), which is
 * what the decoder — and therefore the relay — actually refuses against. A
 * hire this host would have to truncate to seat is refused before it is signed
 * rather than seated against a brief nobody wrote.
 */
export const MAX_CODING_SESSION_HIRE_BRIEF_BYTES =
  MAX_CODING_SESSION_LIFECYCLE_INITIAL_TURN_BYTES -
  CODING_SESSION_HIRE_BRIEF_PREFIX.length;

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

/**
 * The 2026-09-01 form: the seven keys plus `requestedBy`, before `routing`.
 *
 * `requestedBy` is the pubkey of the seat that ran `bee sessions hire`. It is
 * additive and independent of `routing`, so the two amendments multiply rather
 * than replace each other: **four** accepted key sets, exactly the matrix
 * `buzz-core` decodes (`coding_session_lifecycle_command.rs`; REPORT-B1 2.3).
 * A hire that predates either amendment keeps being read forever.
 */
export const CODING_SESSION_HIRE_ATTRIBUTED_ACTION_KEYS = [
  ...CODING_SESSION_HIRE_ACTION_KEYS,
  "requestedBy",
] as const;

/**
 * The 2026-08-30 form: the seven keys plus `routing`, last.
 *
 * Trailing position matches how every other additive amendment on this wire
 * has landed (`sessionRef`, `genesisRef`, the seat pair).
 */
export const CODING_SESSION_HIRE_ROUTED_ACTION_KEYS = [
  ...CODING_SESSION_HIRE_ACTION_KEYS,
  "routing",
] as const;

/** All four accepted key sets, built from the two independent axes. */
export const CODING_SESSION_HIRE_ACTION_KEY_SETS: readonly (readonly string[])[] =
  [
    CODING_SESSION_HIRE_ACTION_KEYS,
    CODING_SESSION_HIRE_ATTRIBUTED_ACTION_KEYS,
    CODING_SESSION_HIRE_ROUTED_ACTION_KEYS,
    [...CODING_SESSION_HIRE_ATTRIBUTED_ACTION_KEYS, "routing"],
  ];

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
  /**
   * The seat that asked for this hire, or absent on a hire that predates the
   * amendment.
   *
   * **An unverified claim, not an attribution.** The relay verifies the
   * event's signature and therefore holds `event.pubkey`, but v1 deliberately
   * does not compare the two — so any signer the relay admits for a hire can
   * name a different seat here (POLICY.md 5). This host closes that gap for
   * itself with {@link codingSessionHireRequesterStanding}; a surface that
   * renders the name without asking is printing a claim as a fact.
   */
  requestedBy?: string;
  /**
   * What the lead knows about the job, for the host's router — or absent on
   * a hire that predates the amendment.
   *
   * Never a model. "The lead chooses the capability required. The router
   * chooses the execution target."
   */
  routing?: CodingSessionHireRoutingRequest;
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

/**
 * Enough of a malformed hire to answer it.
 *
 * A hire this host cannot read is still a hire somebody is waiting on, so the
 * classifier hands back whatever of the envelope *was* readable — the channel,
 * the command id, the signer, and the session it named when that much parsed.
 * Without this the host could only drop it, which is exactly what it did for
 * fifteen minutes on 2026-08-30 (ledger draft 97).
 */
export type CodingSessionHireAddress = {
  channelId: string;
  commandId: string;
  /** Whoever signed it — the seat a refusal is published back to. */
  requesterPubkey: string;
  /** The umbrella it named, when that key was readable. */
  sessionRef: string | null;
  /** The role it asked for, when that key was readable. */
  role: string | null;
};

export type CodingSessionHireClassification =
  | ({ kind: "hire" } & CodingSessionHireRequest)
  | { kind: "irrelevant" }
  | {
      kind: "malformed";
      /** The dotted key that failed, e.g. `action.routing.tier`. */
      failingKey: string;
      /** One sentence naming what is wrong with it. */
      reason: string;
      /** Null when not even the envelope parsed; then nobody can be told. */
      address: CodingSessionHireAddress | null;
    }
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
  /** The seat asking for the hire; absent reproduces the seven-key form. */
  requestedBy?: string;
  /** Present only on a routed hire; absent reproduces the seven-key form. */
  routing?: CodingSessionHireRoutingRequest;
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
      // `undefined` counts as absent on both amendments, so a hire that names
      // neither is byte-identical to the pre-amendment form.
      ...(input.requestedBy === undefined
        ? {}
        : { requestedBy: input.requestedBy }),
      ...(input.routing === undefined ? {} : { routing: input.routing }),
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
  requestedBy?: string;
  routing?: CodingSessionHireRoutingRequest;
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
  if (
    input.requestedBy !== undefined &&
    !isCodingSessionLifecycleHex64(input.requestedBy)
  ) {
    throw new Error("action.requestedBy must be a lowercase 64-hex public key");
  }
  if (input.routing !== undefined) {
    const read = readCodingSessionHireRoutingRequest(input.routing);
    if (!read.ok) {
      throw new Error(`action.${read.key} ${read.why}`);
    }
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
    return unaddressable(
      "tags",
      "the h / csl-v / csl-command tags are not the exact three this wire carries",
    );
  }
  const requesterPubkey = normalizePubkey(event.pubkey);
  if (!requesterPubkey) {
    return unaddressable(
      "pubkey",
      "the signer is not a lowercase 64-hex pubkey",
    );
  }
  if (
    !hasExactKeys(payload, ["schema", "commandId", "action"]) ||
    payload.schema !== CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA ||
    !boundedNonempty(
      payload.commandId,
      MAX_CODING_SESSION_LIFECYCLE_IDENTIFIER_BYTES,
    ) ||
    payload.commandId !== tags[2]
  ) {
    return unaddressable(
      "content",
      `the envelope is not { schema: ${CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA}, commandId, action } with commandId equal to the csl-command tag`,
    );
  }
  const action = payload.action;
  // Addressable from here on: the channel, the command id and the signer all
  // parsed, so whatever is wrong below can be said *to somebody*.
  const address: CodingSessionHireAddress = {
    channelId: tags[0],
    commandId: payload.commandId,
    requesterPubkey,
    sessionRef: isCodingSessionSessionRef(action.sessionRef)
      ? (action.sessionRef as string)
      : null,
    role: isCodingSessionRoleSlug(action.role) ? (action.role as string) : null,
  };
  const malformed = (failingKey: string, reason: string) =>
    ({ kind: "malformed", failingKey, reason, address }) as const;
  // Before the key-set check, so the reader is told which key is in the wrong
  // place rather than that the key set does not match — the same ordering
  // `reject_foreign_additive_key` uses in buzz-core (REVIEW-B1 F1).
  const crossed = describeCrossedCodingSessionLifecycleKey(action);
  if (crossed) return malformed("action.hireRef", crossed);
  const routed = Object.hasOwn(action, "routing");
  const attributed = Object.hasOwn(action, "requestedBy");
  if (
    !CODING_SESSION_HIRE_ACTION_KEY_SETS.some((keys) =>
      hasExactKeys(action, [...keys]),
    )
  ) {
    return malformed("action", describeHireKeySet(action));
  }
  if (routed) {
    const read = readCodingSessionHireRoutingRequest(action.routing);
    if (!read.ok) {
      return malformed(`action.${read.key}`, read.why);
    }
  }
  for (const [key, ok, why] of [
    [
      "action.sessionRef",
      isCodingSessionSessionRef(action.sessionRef),
      "must be a canonical lowercase hyphenated UUID",
    ],
    [
      "action.genesisRef",
      typeof action.genesisRef === "string" &&
        /^[0-9a-f]{64}$/.test(action.genesisRef),
      "must be a lowercase 64-hex event id",
    ],
    [
      "action.role",
      isCodingSessionRoleSlug(action.role),
      `must be [a-z0-9-]+ and at most ${MAX_CODING_SESSION_ROLE_BYTES} bytes`,
    ],
    [
      "action.providerInstanceRef",
      isOptionalReference(action.providerInstanceRef),
      "must be a non-empty runtime ref, or null",
    ],
    [
      "action.model",
      isOptionalReference(action.model),
      "must be a non-empty catalog id, or null",
    ],
    [
      "action.brief",
      boundedNonempty(action.brief, MAX_CODING_SESSION_HIRE_BRIEF_BYTES),
      `must be non-empty and at most ${MAX_CODING_SESSION_HIRE_BRIEF_BYTES} bytes`,
    ],
    [
      // Absent is not null, and uppercase is not folded: a hire that carries
      // the key carries a canonical pubkey or it is malformed.
      "action.requestedBy",
      !attributed || isCodingSessionLifecycleHex64(action.requestedBy),
      "must be a lowercase 64-hex public key",
    ],
  ] as const) {
    if (!ok) return malformed(key, why);
  }
  // Last, because it is the expensive one and every cheap refusal above has
  // already run.
  if (!hasValidSignature(event)) return { kind: "invalid-signature" };
  return {
    kind: "hire",
    eventId: event.id,
    channelId: address.channelId,
    commandId: address.commandId,
    requesterPubkey,
    createdAt: event.created_at,
    // Each field was checked by the table above; the loop that ran it costs
    // the narrowing the old one-expression guard gave for free.
    action: {
      type: CODING_SESSION_HIRE_ACTION_TYPE,
      sessionRef: action.sessionRef as string,
      genesisRef: action.genesisRef as string,
      role: action.role as string,
      providerInstanceRef: action.providerInstanceRef as string | null,
      model: action.model as string | null,
      brief: action.brief as string,
      ...(attributed ? { requestedBy: action.requestedBy as string } : {}),
      ...(routed
        ? { routing: action.routing as CodingSessionHireRoutingRequest }
        : {}),
    },
  };
}

/**
 * How much this host may say about who asked for a seat.
 *
 * Three answers, never two. `attributed` is the only one that licenses a name
 * on screen: the hire claimed a requester **and** that claim equals the key
 * that signed the event, which is a comparison this host makes for itself
 * rather than trusting the relay to have made (it does not — POLICY.md 5).
 * `unclaimed` is a hire from before the amendment: unknown, never mismatched.
 * `disputed` is a hire naming somebody other than its own signer, which is
 * exactly the forgery v1 leaves open, and it is disclosed rather than dropped
 * — a seat still needs answering, and the person still needs telling.
 */
export type CodingSessionHireRequesterStanding =
  | { kind: "attributed"; requesterPubkey: string }
  | { kind: "unclaimed"; requesterPubkey: string }
  | { kind: "disputed"; claimedPubkey: string; signerPubkey: string };

/** Compare a hire's claim with its own signer. */
export function codingSessionHireRequesterStanding(request: {
  requesterPubkey: string;
  action: { requestedBy?: string };
}): CodingSessionHireRequesterStanding {
  const signer = request.requesterPubkey.trim().toLowerCase();
  const claimed = request.action.requestedBy;
  if (claimed === undefined) {
    return { kind: "unclaimed", requesterPubkey: signer };
  }
  return claimed === signer
    ? { kind: "attributed", requesterPubkey: signer }
    : { kind: "disputed", claimedPubkey: claimed, signerPubkey: signer };
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

/** A malformed hire nobody can be told about: the envelope itself did not parse. */
function unaddressable(
  failingKey: string,
  reason: string,
): CodingSessionHireClassification {
  return { kind: "malformed", failingKey, reason, address: null };
}

/**
 * Name the keys that made an action neither the seven-key nor the eight-key
 * form.
 *
 * Both sides of the difference are printed. "unknown key routing" alone would
 * be wrong for a payload that is also missing `brief`, and a lead reading only
 * half of what is wrong fixes half of it.
 */
function describeHireKeySet(action: Record<string, unknown>): string {
  const allowed = new Set<string>(
    CODING_SESSION_HIRE_ACTION_KEY_SETS.flatMap((keys) => [...keys]),
  );
  const unknown = Object.keys(action).filter((key) => !allowed.has(key));
  const missing = CODING_SESSION_HIRE_ACTION_KEYS.filter(
    (key) => !Object.hasOwn(action, key),
  );
  const parts: string[] = [];
  if (unknown.length > 0) parts.push(`unknown key(s) ${unknown.join(", ")}`);
  if (missing.length > 0) parts.push(`missing key(s) ${missing.join(", ")}`);
  return `${
    parts.length > 0 ? parts.join("; ") : "the key set does not match"
  } — a session.hire carries ${CODING_SESSION_HIRE_ACTION_KEYS.join(
    ", ",
  )} plus an optional requestedBy and an optional routing`;
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
