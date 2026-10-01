import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_CODING_SESSION_COMMAND } from "@/shared/constants/kinds";

/** The locked public payload schema for coding-session commands. */
export const CODING_SESSION_COMMAND_SCHEMA = "buzz-coding-session-command/v1";
/** The locked version of the public command tag envelope. */
export const CODING_SESSION_COMMAND_TAG_VERSION = "csc1-1";
/**
 * Nostr tag name marking a `thread.turn.start` command as its own signer's
 * answer to something already decided — a hire refusal, for instance — never
 * a person's words.
 *
 * Mirrors `CODING_SESSION_HOST_ANSWER_TAG_NAME` in
 * `crates/buzz-core/src/coding_session_payload.rs`, which is also where the
 * provider-side predicate that reads it lives. The relay's envelope
 * validator (`validate_coding_session_command_envelope`,
 * `crates/buzz-relay/src/handlers/ingest.rs`) allows exactly one of these,
 * with the value below; the command must still be published exactly as any
 * other turn — `bee sessions hire` reads this same event directly off the
 * relay to answer its own poll (ledger 178(b)), so it is the CLI's answer
 * channel and never a redundant echo to suppress.
 */
export const CODING_SESSION_HOST_ANSWER_TAG_NAME = "buzz-host-answer";
/** The one recognized host-answer tag value today. */
export const CODING_SESSION_HOST_ANSWER_TAG_HIRE = "hire";

/**
 * The relay's ingest rejection reason, unprefixed, when its allowlist
 * predates this tag entirely (ledger 192, 2026-09-20; matching narrowed to
 * exact equality in ledger 200 after an adversarial review, finding 8).
 *
 * Lane 181 taught `validate_coding_session_command_envelope`
 * (`crates/buzz-relay/src/handlers/ingest.rs`) to accept exactly one
 * `buzz-host-answer` tag with the value `hire`, but a relay built before that
 * lane still ends its match arm `_ => return Err("unsupported coding-session
 * command tag")`. Ingest wraps that into the publish rejection this client
 * sees, verbatim, as `invalid: unsupported coding-session command tag`
 * (`IngestError::Rejected(format!("invalid: {error}"))`,
 * `crates/buzz-relay/src/handlers/ingest.rs`, the
 * `KIND_CODING_SESSION_COMMAND` arm) — `relayClientSession.ts`'s `handleOk`
 * rejects the pending publish with that exact string as `Error.message`, no
 * further wrapping.
 *
 * A sibling rejection in the same validator, `"unsupported coding-session
 * command tag version"` (the `cs-v` arm, a few lines above the one this
 * constant names), contains this string as a substring — a stale `cs-v`
 * value, not an unknown tag — so a substring match wrongly matched it too
 * and triggered the same untagged retry, harmlessly (the retry fails for the
 * same reason) but not narrowly. {@link
 * isCodingSessionHostAnswerTagUnsupportedRejection} strips the rejection's
 * `<word>: ` classifier prefix and compares what remains for exact equality
 * instead.
 */
export const CODING_SESSION_HOST_ANSWER_TAG_UNSUPPORTED_MESSAGE =
  "unsupported coding-session command tag";

/**
 * Strip the leading `<word>: ` classifier every relay rejection reason
 * carries (`invalid: `, `restricted: `, `blocked: `, `rate-limited: `,
 * `duplicate: `, `error: `, `auth-required: ` — the same vocabulary ledger
 * 170 names for the outbox's own final/retryable split), and surrounding
 * whitespace. A message with no such prefix is returned trimmed and
 * unchanged, so a rejection this client did not expect still compares
 * safely rather than throwing.
 */
function stripRelayRejectionPrefix(message: string): string {
  return message
    .trim()
    .replace(/^[a-z][a-z-]*:\s*/i, "")
    .trim();
}

/**
 * True for exactly the rejection above — an older relay's allowlist refusing
 * the tag itself — never the tag-version rejection, and never any other
 * reason a tagged host-answer turn could be refused (a bad signature, a
 * membership gate, a network failure, a generic rate limit).
 */
export function isCodingSessionHostAnswerTagUnsupportedRejection(
  error: unknown,
): boolean {
  return (
    error instanceof Error &&
    stripRelayRejectionPrefix(error.message) ===
      CODING_SESSION_HOST_ANSWER_TAG_UNSUPPORTED_MESSAGE
  );
}
/**
 * Whether a turn was refused because this relay's attachment allowlist predates
 * text attachments.
 *
 * A relay validates the 44220 payload on ingest with the same
 * `CodingSessionCommandPayload::validate` the client mirrors, so a relay built
 * before `text/plain` joined `ALLOWED_ATTACHMENT_MIMES` rejects a pasted file
 * with `invalid: action.attachments[0].mime must be one of image/jpeg,
 * image/png, image/gif, image/webp` — a sentence that reads, to the person who
 * pasted a log, as though they had done something wrong.
 *
 * Identified by what the list the relay named is *missing*, not by its exact
 * text: that is the one fact that distinguishes an old relay from a genuinely
 * bad MIME, and it survives the list growing again later. A rejection naming a
 * list that does include `text/plain` is some other problem and must keep its
 * own message.
 */
export function isCodingSessionTextAttachmentUnsupportedRejection(
  error: unknown,
): boolean {
  if (!(error instanceof Error)) return false;
  const reason = stripRelayRejectionPrefix(error.message);
  const match = /^action\.attachments\[\d+]\.mime must be one of (.+)$/.exec(
    reason,
  );
  if (match === null) return false;
  return !match[1]
    .split(",")
    .map((mime) => mime.trim())
    .includes(CODING_SESSION_TEXT_ATTACHMENT_MIME);
}

/** What to tell the person when the relay above refused their paste. */
export const CODING_SESSION_TEXT_ATTACHMENT_UNSUPPORTED_MESSAGE =
  "This community's relay does not accept pasted files yet, so the turn was not sent. Remove the pasted file to send the message, or ask for the relay to be updated.";

/** Maximum UTF-8 byte length for a command or target identifier. */
export const MAX_CODING_SESSION_IDENTIFIER_BYTES = 256;
/** Maximum UTF-8 byte length for a coding-session turn. */
export const MAX_CODING_SESSION_TEXT_BYTES = 12 * 1024;
/** Mirrors `MAX_TURN_ATTACHMENTS` in buzz-core — images and text together. */
export const MAX_CODING_SESSION_ATTACHMENTS = 4;
/** Mirrors `MAX_TURN_ATTACHMENT_BYTES` in buzz-core; images only. */
export const MAX_CODING_SESSION_ATTACHMENT_BYTES = 10 * 1024 * 1024;
/**
 * Mirrors `MAX_TURN_TEXT_ATTACHMENT_BYTES` in buzz-core.
 *
 * Far below the image bound, and for a different reason: an image is downscaled
 * before it reaches a model and text is not, so every byte here is spent out of
 * a context window. It is still ~85× what a turn's own
 * {@link MAX_CODING_SESSION_TEXT_BYTES} can hold, which is the whole point of
 * attaching a paste rather than inlining it.
 */
export const MAX_CODING_SESSION_TEXT_ATTACHMENT_BYTES = 1024 * 1024;
/** Mirrors `ALLOWED_IMAGE_ATTACHMENT_MIMES` in buzz-core. */
export const CODING_SESSION_IMAGE_ATTACHMENT_MIMES = [
  "image/jpeg",
  "image/png",
  "image/gif",
  "image/webp",
] as const;
/**
 * The one MIME a text attachment declares, mirroring
 * `ALLOWED_TEXT_ATTACHMENT_MIMES` in buzz-core.
 *
 * It is also what the relay's generic-file validator stores an un-sniffable
 * UTF-8 upload as, so the declared MIME, the sidecar and the `.txt` the
 * provider fetches by all agree. They have to: the blob route answers `404`
 * when the requested extension is not the sidecar's canonical one.
 */
export const CODING_SESSION_TEXT_ATTACHMENT_MIME = "text/plain";
/** Mirrors `ALLOWED_ATTACHMENT_MIMES` in buzz-core: the images then the text. */
export const CODING_SESSION_ATTACHMENT_MIMES = [
  ...CODING_SESSION_IMAGE_ATTACHMENT_MIMES,
  CODING_SESSION_TEXT_ATTACHMENT_MIME,
] as const;

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

/**
 * One image or text file attached to a turn, addressed by its Blossom hash.
 *
 * No URL, deliberately: the provider derives `{relay}/media/{sha256}.{ext}`
 * from the relay it is already connected to, so a signed command can never
 * point it at somewhere else. Mirrors `TurnAttachment` in
 * `crates/buzz-core/src/coding_session_command.rs`.
 */
export type CodingSessionTurnAttachment = {
  sha256: string;
  mime: string;
  size: number;
  dim?: string;
  filename?: string;
};

/**
 * The wire `type` of a CI-continuation registration.
 *
 * Registering is not sending a turn: nothing is queued, no budget is spent,
 * and the provider answers with a `continuation_registered` receipt. A turn
 * follows only if the named run attempt records a result before `expiresAt`
 * and the signer may still steer the target then. Desktop does not publish
 * these today — `bee ci continue` does — but every reader of a channel's
 * 44220s has to know it is a real action rather than a malformed turn.
 */
export const CODING_SESSION_CI_CONTINUATION_ACTION_TYPE =
  "thread.turn.continue_on_ci";

/** Maximum UTF-8 byte length of a CI continuation prompt. */
export const MAX_CODING_SESSION_CONTINUATION_BYTES =
  MAX_CODING_SESSION_TEXT_BYTES;

/**
 * Exact identity of one external CI run attempt, mirroring `CiResultIdentity`
 * in `crates/buzz-core/src/ci_result.rs`.
 *
 * All eight keys are required and none may be added: the canonical JSON of
 * exactly these fields is the correlation digest a recorded result is filed
 * under, so a ninth key here would name a run no result could ever satisfy.
 */
export type CodingSessionCiResultIdentity = {
  project: string;
  repository: string;
  commit: string;
  check: string;
  run: string;
  attempt: number;
  workflow: string;
  phase: "build" | "deploy";
};

/** The eight keys a {@link CodingSessionCiResultIdentity} carries, in order. */
export const CODING_SESSION_CI_IDENTITY_KEYS = [
  "project",
  "repository",
  "commit",
  "check",
  "run",
  "attempt",
  "workflow",
  "phase",
] as const;

/** Actions supported by the governed coding-session command contract. */
export type CodingSessionCommandAction =
  | {
      type: "thread.turn.start";
      text: string;
      /**
       * Written only when the turn actually carries images, for exactly the
       * reason `deliver` is omitted at its default: the payload is validated
       * with `deny_unknown_fields` at the relay *and* the provider, so a turn
       * with no images has to serialize to the bytes it always did. A turn
       * that does carry them requires a relay and provider that know the
       * field — which is what the execution's `promptImage` capability
       * tells the composer before it offers the control.
       */
      attachments?: CodingSessionTurnAttachment[];
      /**
       * Written only when the sender asks for something other than the wire
       * default. `boundary` is the default an absent key already means, and a
       * relay that predates this field validates the payload with
       * `deny_unknown_fields` — so spelling the default out turns every
       * ordinary turn into a refusal against a relay that has not shipped the
       * field yet. `steer` and `interrupt` are written, and they require a
       * relay that knows the field.
       */
      deliver?: CodingSessionTurnDelivery;
    }
  | {
      type: "thread.turn.interrupt";
    }
  | {
      type: typeof CODING_SESSION_CI_CONTINUATION_ACTION_TYPE;
      /** The exact CI run attempt whose recorded result unblocks the turn. */
      identity: CodingSessionCiResultIdentity;
      /** Text delivered with the verified result when the turn starts. */
      continuation: string;
      /**
       * Unix seconds after which the registration is refused rather than
       * delivered. Required and positive — there is no "waits forever".
       */
      expiresAt: number;
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
  attachments?: CodingSessionTurnAttachment[];
  deliver: CodingSessionTurnDelivery;
  /**
   * Marks this turn as the sender's own answer to something it already
   * decided, never a person's words — see
   * {@link CODING_SESSION_HOST_ANSWER_TAG_NAME}. Omitted by every ordinary
   * turn; the hire host is this tag's only writer today.
   */
  hostAnswer?: boolean;
}): CodingSessionCommandEventInput {
  // The caller always names a class; an unreadable one is refused here rather
  // than defaulted, so a turn asked to interrupt is never quietly delivered at
  // a boundary because of a typo.
  if (!isCodingSessionTurnDelivery(input.deliver)) {
    throw new Error(
      `action.deliver must be one of ${CODING_SESSION_TURN_DELIVERIES.join(", ")}`,
    );
  }
  return buildCodingSessionActionEvent({
    channelId: input.channelId,
    commandId: input.commandId,
    target: input.target,
    hostAnswer: input.hostAnswer,
    action: {
      type: "thread.turn.start",
      text: input.text,
      // Both optional keys are omitted at their defaults, for the same
      // forward-compatibility reason — see their doc comments. Spreading
      // rather than assigning `undefined` keeps them out of `JSON.stringify`
      // output entirely.
      ...(input.attachments && input.attachments.length > 0
        ? { attachments: input.attachments }
        : {}),
      ...(input.deliver === "boundary" ? {} : { deliver: input.deliver }),
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
  hostAnswer?: boolean;
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
      ...(input.hostAnswer === true
        ? [
            [
              CODING_SESSION_HOST_ANSWER_TAG_NAME,
              CODING_SESSION_HOST_ANSWER_TAG_HIRE,
            ],
          ]
        : []),
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
        attachments?: unknown;
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
    // Absent is legal — it is how this client writes `boundary`. Anything
    // present but unrecognised is refused here rather than sent and defaulted
    // by the provider: a turn the sender asked to interrupt with must never be
    // quietly delivered at a boundary because a typo made it unreadable.
    if (
      input.action.deliver !== undefined &&
      !isCodingSessionTurnDelivery(input.action.deliver)
    ) {
      throw new Error(
        `action.deliver must be one of ${CODING_SESSION_TURN_DELIVERIES.join(", ")}`,
      );
    }
    // Same rule as `deliver`: absent is legal, present-but-malformed is
    // refused before signing. The relay and provider both re-check this, but
    // failing here is what lets the composer say which image was wrong.
    if (input.action.attachments !== undefined) {
      validateCodingSessionAttachments(input.action.attachments);
    }
  }
  // A registration is checked here for the same reason a turn is: the bounds
  // the relay and provider enforce should be named by whoever is about to
  // sign, not discovered as a rejection afterwards.
  if (input.action.type === CODING_SESSION_CI_CONTINUATION_ACTION_TYPE) {
    if (!isCodingSessionCiContinuationAction(input.action)) {
      throw new Error(
        `action must carry exactly type, identity, continuation, and expiresAt, with an identity of ${CODING_SESSION_CI_IDENTITY_KEYS.join(", ")}`,
      );
    }
  }
}

/**
 * True for exactly the CI-continuation action shape, and nothing adjacent.
 *
 * Strict on both sides: the action carries exactly `type`, `identity`,
 * `continuation`, `expiresAt`, and the identity exactly its eight keys — the
 * relay and the provider both decode this payload with
 * `deny_unknown_fields`, so a reader that tolerated an extra key here would
 * be reading terms nobody downstream will honour. It is deliberately a
 * *recognizer*, not a validator of the identity's contents: whether the
 * commit is 40-hex and the workflow a canonical UUID is `buzz-core`'s rule,
 * re-implemented here only where it would change what a surface renders.
 */
export function isCodingSessionCiContinuationAction(
  value: unknown,
): value is Extract<
  CodingSessionCommandAction,
  { type: typeof CODING_SESSION_CI_CONTINUATION_ACTION_TYPE }
> {
  if (!isPlainRecord(value)) return false;
  if (value.type !== CODING_SESSION_CI_CONTINUATION_ACTION_TYPE) return false;
  if (
    !hasExactCommandKeys(value, [
      "type",
      "identity",
      "continuation",
      "expiresAt",
    ])
  ) {
    return false;
  }
  if (
    typeof value.continuation !== "string" ||
    value.continuation.trim().length === 0 ||
    new TextEncoder().encode(value.continuation).byteLength >
      MAX_CODING_SESSION_CONTINUATION_BYTES
  ) {
    return false;
  }
  if (
    !Number.isSafeInteger(value.expiresAt) ||
    (value.expiresAt as number) <= 0
  ) {
    return false;
  }
  const identity = value.identity;
  if (
    !isPlainRecord(identity) ||
    !hasExactCommandKeys(identity, [...CODING_SESSION_CI_IDENTITY_KEYS])
  ) {
    return false;
  }
  for (const key of [
    "project",
    "repository",
    "commit",
    "check",
    "run",
    "workflow",
  ]) {
    const field = identity[key];
    if (typeof field !== "string" || field.length === 0) return false;
  }
  return (
    Number.isSafeInteger(identity.attempt) &&
    (identity.attempt as number) > 0 &&
    (identity.phase === "build" || identity.phase === "deploy")
  );
}

function isPlainRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasExactCommandKeys(
  value: Record<string, unknown>,
  keys: readonly string[],
): boolean {
  const actual = Object.keys(value);
  return actual.length === keys.length && keys.every((key) => key in value);
}

/** Bounds every attachment field the relay will re-check after signing. */
function validateCodingSessionAttachments(value: unknown): void {
  if (!Array.isArray(value)) {
    throw new Error("action.attachments must be an array");
  }
  if (value.length > MAX_CODING_SESSION_ATTACHMENTS) {
    throw new Error(
      `action.attachments exceeds ${MAX_CODING_SESSION_ATTACHMENTS} entries`,
    );
  }
  value.forEach((entry, index) => {
    const attachment = entry as Partial<CodingSessionTurnAttachment>;
    if (
      typeof attachment?.sha256 !== "string" ||
      !/^[0-9a-f]{64}$/.test(attachment.sha256)
    ) {
      throw new Error(
        `action.attachments[${index}].sha256 must be 64 lowercase hex characters`,
      );
    }
    if (
      typeof attachment.mime !== "string" ||
      !(CODING_SESSION_ATTACHMENT_MIMES as readonly string[]).includes(
        attachment.mime,
      )
    ) {
      throw new Error(
        `action.attachments[${index}].mime must be one of ${CODING_SESSION_ATTACHMENT_MIMES.join(", ")}`,
      );
    }
    // Each kind against its own ceiling, exactly as `TurnAttachment::validate`
    // does, and the message names the bound that refused it: a text attachment
    // turned away at 1 MiB and an image accepted at 9 MiB are the same field,
    // and "too large" alone would point a person at the wrong rule.
    const limit =
      attachment.mime === CODING_SESSION_TEXT_ATTACHMENT_MIME
        ? MAX_CODING_SESSION_TEXT_ATTACHMENT_BYTES
        : MAX_CODING_SESSION_ATTACHMENT_BYTES;
    if (
      !Number.isSafeInteger(attachment.size) ||
      (attachment.size ?? 0) <= 0 ||
      (attachment.size ?? 0) > limit
    ) {
      throw new Error(
        `action.attachments[${index}].size must be between 1 and ${limit} bytes`,
      );
    }
  });
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
