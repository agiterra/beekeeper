import type { RelayEvent } from "@/shared/api/types";
import { KIND_CODING_SESSION_TEAM_TRANSACTION } from "@/shared/constants/kinds";
import { hasValidSignature } from "@/shared/lib/authors";
import {
  hasDuplicateJsonKeys,
  hasExactFields,
} from "@/shared/coordination/sessionCoordinationStrictJson";

export { KIND_CODING_SESSION_TEAM_TRANSACTION };
export const CODING_SESSION_TEAM_TRANSACTION_SCHEMA =
  "buzz-coding-session-team-transaction/v1";

const encoder = new TextEncoder();
const HEX64 = /^[0-9a-f]{64}$/;
const GIT_SHA = /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/;
const ROLE = /^[a-z0-9-]{1,64}$/;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
/**
 * Every operation type a signed 44244 may carry.
 *
 * Exported because the surfaces that render these records key tables on them,
 * and a table that quietly misses one renders `undefined`. A test holds this
 * list against the Route's own glyph and word tables, so adding a verb here
 * without teaching the surface its sign fails in CI rather than in Mission.
 */
export const CODING_SESSION_TEAM_TRANSACTION_TYPES = [
  "assignment",
  "report",
  "verdict",
  "acknowledgement",
  "mission.completed",
  "mission.blocked",
  "note",
  "decision.request",
  "decision.answer",
] as const satisfies readonly CodingSessionTeamTransactionPayload["type"][];

const TYPES: ReadonlySet<string> = new Set(
  CODING_SESSION_TEAM_TRANSACTION_TYPES,
);

type Unlisted<T extends never> = T;
/**
 * Compile-time proof the list above covers the payload union. A new member of
 * {@link CodingSessionTeamTransactionPayload}'s `type` that is not listed makes
 * this alias an error, here, rather than a record the decoder refuses.
 */
export type CodingSessionTeamTransactionTypesAreComplete = Unlisted<
  Exclude<
    CodingSessionTeamTransactionPayload["type"],
    (typeof CODING_SESSION_TEAM_TRANSACTION_TYPES)[number]
  >
>;
const DECISION_FOUNDER = "founder";
const MAX_NOTE_REFS = 16;
const MAX_DECISION_OPTIONS = 8;
const MAX_DECISION_OPTION_BYTES = 512;
const MAX_DECISION_BLOCKS = 16;
const MAX_SHORT_TEXT_BYTES = 2 * 1024;

export type StrictDecodeResult<T> =
  | { ok: true; value: T }
  | { ok: false; error: string };

export type CodingSessionTeamTest = {
  name: string;
  command: string;
  outcome: "passed" | "failed" | "not-run";
  evidence: string | null;
};

export type CodingSessionTeamTransactionBody =
  | {
      assigneeActor: string;
      assigneeRole: string;
      objective: string;
      brief: string;
      branch: string | null;
      baseSha: string | null;
      fileOwnership: string[];
      acceptanceSteps: string[];
    }
  | {
      assignmentRef: string;
      summary: string;
      branch: string | null;
      baseSha: string | null;
      headSha: string | null;
      files: string[];
      tests: CodingSessionTeamTest[];
      redBeforeGreen: boolean | null;
      deviations: string[];
      residuals: string[];
      anomalies: string[];
    }
  | {
      subtype: "refutation";
      assignmentRef: string;
      reportRef: string;
      decision: "confirmed" | "not-refuted" | "blocked";
      summary: string;
      findings: string[];
      requiredAction: string | null;
    }
  | {
      subtype: "disposition";
      assignmentRef: string;
      reportRef: string;
      refutationRef: string | null;
      decision:
        | "approve"
        | "approve-with-notes"
        | "changes-requested"
        | "reject"
        | "blocked";
      summary: string;
      findings: string[];
      requiredAction: string | null;
    }
  | {
      acknowledgedEventRef: string;
      status: "received";
      note: string | null;
    }
  | {
      assignmentRefs: string[];
      landedShas: string[];
      summary: string;
      followUps: string[];
    }
  | {
      assignmentRefs: string[];
      summary: string;
      blockers: string[];
      heldOn: string | null;
      requiredAction: string;
    }
  | {
      /** Something said. Never a phase, never a terminal. */
      text: string;
      /** Pointers for a reader; not causal and not required to resolve. */
      refs: string[];
    }
  | {
      question: string;
      options: string[];
      /** Exactly `founder`, or the 64-hex actor holding the decision. */
      heldOn: string;
      blocks: string[];
      recommendation: string | null;
    }
  | {
      requestRef: string;
      /** An index into the request's options, or bounded free text. */
      choice: number | string;
      note: string | null;
    };

export type CodingSessionTeamTransactionPayload = {
  schema: typeof CODING_SESSION_TEAM_TRANSACTION_SCHEMA;
  sessionRef: string;
  genesisRef: string;
  type:
    | "assignment"
    | "report"
    | "verdict"
    | "acknowledgement"
    | "mission.completed"
    | "mission.blocked"
    | "note"
    | "decision.request"
    | "decision.answer";
  supersedes: string | null;
  deliveryCommandId: string | null;
  body: CodingSessionTeamTransactionBody;
};

export type VerifiedCodingSessionTeamTransaction = {
  eventId: string;
  authorPubkey: string;
  createdAt: number;
  payload: CodingSessionTeamTransactionPayload;
  /** Exact signed event projected for the native canonical fold. */
  wireEvent: RelayEvent;
};

function fail<T>(error: string): StrictDecodeResult<T> {
  return { ok: false, error };
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function byteLength(value: string): number {
  return encoder.encode(value).length;
}

function isText(value: unknown, maxBytes = 8 * 1024): value is string {
  return (
    typeof value === "string" &&
    value.trim().length > 0 &&
    !value.includes("\0") &&
    byteLength(value) <= maxBytes
  );
}

function isNullableText(value: unknown, maxBytes = 8 * 1024): boolean {
  return value === null || isText(value, maxBytes);
}

function isStringArray(
  value: unknown,
  options: {
    maxItems?: number;
    maxBytes?: number;
    nonempty?: boolean;
    unique?: boolean;
  } = {},
): value is string[] {
  if (!Array.isArray(value)) return false;
  const maxItems = options.maxItems ?? 256;
  if (
    value.length > maxItems ||
    (options.nonempty === true && value.length === 0) ||
    !value.every((item) => isText(item, options.maxBytes))
  ) {
    return false;
  }
  return options.unique !== true || new Set(value).size === value.length;
}

function isReferenceArray(
  value: unknown,
  nonempty: boolean,
): value is string[] {
  return (
    isStringArray(value, { nonempty, unique: true }) &&
    value.every((item) => HEX64.test(item))
  );
}

function nullableReference(value: unknown): boolean {
  return value === null || (typeof value === "string" && HEX64.test(value));
}

function nullableSha(value: unknown): boolean {
  return value === null || (typeof value === "string" && GIT_SHA.test(value));
}

function validateAssignment(body: Record<string, unknown>): boolean {
  return (
    hasExactFields(body, [
      [
        "assigneeActor",
        "assigneeRole",
        "objective",
        "brief",
        "branch",
        "baseSha",
        "fileOwnership",
        "acceptanceSteps",
      ],
    ]) &&
    typeof body.assigneeActor === "string" &&
    HEX64.test(body.assigneeActor) &&
    typeof body.assigneeRole === "string" &&
    ROLE.test(body.assigneeRole) &&
    isText(body.objective) &&
    isText(body.brief, 32 * 1024) &&
    isNullableText(body.branch, 255) &&
    nullableSha(body.baseSha) &&
    isStringArray(body.fileOwnership, {
      maxBytes: 1024,
      unique: true,
    }) &&
    isStringArray(body.acceptanceSteps, { nonempty: true })
  );
}

function validateReportTest(value: unknown): value is CodingSessionTeamTest {
  return (
    hasExactFields(value, [["name", "command", "outcome", "evidence"]]) &&
    isText(value.name, 512) &&
    isText(value.command) &&
    (value.outcome === "passed" ||
      value.outcome === "failed" ||
      value.outcome === "not-run") &&
    isNullableText(value.evidence)
  );
}

function validateReport(body: Record<string, unknown>): boolean {
  return (
    hasExactFields(body, [
      [
        "assignmentRef",
        "summary",
        "branch",
        "baseSha",
        "headSha",
        "files",
        "tests",
        "redBeforeGreen",
        "deviations",
        "residuals",
        "anomalies",
      ],
    ]) &&
    typeof body.assignmentRef === "string" &&
    HEX64.test(body.assignmentRef) &&
    isText(body.summary) &&
    isNullableText(body.branch, 255) &&
    nullableSha(body.baseSha) &&
    nullableSha(body.headSha) &&
    isStringArray(body.files, { maxBytes: 1024, unique: true }) &&
    Array.isArray(body.tests) &&
    body.tests.length <= 128 &&
    body.tests.every(validateReportTest) &&
    (body.redBeforeGreen === null ||
      typeof body.redBeforeGreen === "boolean") &&
    isStringArray(body.deviations) &&
    isStringArray(body.residuals) &&
    isStringArray(body.anomalies)
  );
}

function validateVerdict(body: Record<string, unknown>): boolean {
  const common =
    typeof body.assignmentRef === "string" &&
    HEX64.test(body.assignmentRef) &&
    typeof body.reportRef === "string" &&
    HEX64.test(body.reportRef) &&
    body.assignmentRef !== body.reportRef &&
    isText(body.summary) &&
    isStringArray(body.findings) &&
    isNullableText(body.requiredAction);
  if (!common) return false;
  if (body.subtype === "refutation") {
    return (
      hasExactFields(body, [
        [
          "subtype",
          "assignmentRef",
          "reportRef",
          "decision",
          "summary",
          "findings",
          "requiredAction",
        ],
      ]) &&
      (body.decision === "confirmed" ||
        body.decision === "not-refuted" ||
        body.decision === "blocked")
    );
  }
  return (
    body.subtype === "disposition" &&
    hasExactFields(body, [
      [
        "subtype",
        "assignmentRef",
        "reportRef",
        "refutationRef",
        "decision",
        "summary",
        "findings",
        "requiredAction",
      ],
    ]) &&
    nullableReference(body.refutationRef) &&
    body.refutationRef !== body.assignmentRef &&
    body.refutationRef !== body.reportRef &&
    [
      "approve",
      "approve-with-notes",
      "changes-requested",
      "reject",
      "blocked",
    ].includes(body.decision as string)
  );
}

function isBoundedReferenceArray(value: unknown, maxItems: number): boolean {
  return (
    Array.isArray(value) &&
    value.length <= maxItems &&
    value.every((item) => typeof item === "string" && HEX64.test(item)) &&
    new Set(value).size === value.length
  );
}

function validateNote(body: Record<string, unknown>): boolean {
  return (
    hasExactFields(body, [["text", "refs"]]) &&
    isText(body.text) &&
    isBoundedReferenceArray(body.refs, MAX_NOTE_REFS)
  );
}

function validateDecisionRequest(body: Record<string, unknown>): boolean {
  return (
    hasExactFields(body, [
      ["question", "options", "heldOn", "blocks", "recommendation"],
    ]) &&
    isText(body.question) &&
    isStringArray(body.options, {
      maxItems: MAX_DECISION_OPTIONS,
      maxBytes: MAX_DECISION_OPTION_BYTES,
      unique: true,
    }) &&
    typeof body.heldOn === "string" &&
    (body.heldOn === DECISION_FOUNDER || HEX64.test(body.heldOn)) &&
    isBoundedReferenceArray(body.blocks, MAX_DECISION_BLOCKS) &&
    isNullableText(body.recommendation, MAX_SHORT_TEXT_BYTES)
  );
}

function validateDecisionAnswer(body: Record<string, unknown>): boolean {
  const choiceIsIndex =
    typeof body.choice === "number" &&
    Number.isSafeInteger(body.choice) &&
    body.choice >= 0 &&
    body.choice < MAX_DECISION_OPTIONS;
  return (
    hasExactFields(body, [["requestRef", "choice", "note"]]) &&
    typeof body.requestRef === "string" &&
    HEX64.test(body.requestRef) &&
    (choiceIsIndex || isText(body.choice, MAX_SHORT_TEXT_BYTES)) &&
    isNullableText(body.note)
  );
}

/** Exact refusal for a `mission.blocked` correction that names no blocker. */
export const TERMINAL_CANNOT_CLEAR_ITSELF =
  "use a note or a decision.answer to clear a blocker; a terminal cannot clear itself";

/**
 * Refuse the two supersession shapes that carry no honest meaning, in the same
 * order buzz-core refuses them so both surfaces name the same reason.
 */
function supersessionRefusal(
  type: string,
  supersedes: unknown,
  body: unknown,
): string | null {
  if (supersedes === null) return null;
  if (type === "note") return "a note never supersedes another record";
  if (
    type === "mission.blocked" &&
    isObject(body) &&
    Array.isArray(body.blockers) &&
    body.blockers.length === 0
  ) {
    return TERMINAL_CANNOT_CLEAR_ITSELF;
  }
  return null;
}

function validateBody(type: string, body: unknown): boolean {
  if (!isObject(body)) return false;
  if (type === "assignment") return validateAssignment(body);
  if (type === "report") return validateReport(body);
  if (type === "verdict") return validateVerdict(body);
  if (type === "note") return validateNote(body);
  if (type === "decision.request") return validateDecisionRequest(body);
  if (type === "decision.answer") return validateDecisionAnswer(body);
  if (type === "acknowledgement") {
    return (
      hasExactFields(body, [["acknowledgedEventRef", "status", "note"]]) &&
      typeof body.acknowledgedEventRef === "string" &&
      HEX64.test(body.acknowledgedEventRef) &&
      body.status === "received" &&
      isNullableText(body.note)
    );
  }
  if (type === "mission.completed") {
    return (
      hasExactFields(body, [
        ["assignmentRefs", "landedShas", "summary", "followUps"],
      ]) &&
      isReferenceArray(body.assignmentRefs, true) &&
      isStringArray(body.landedShas, { unique: true }) &&
      body.landedShas.every((sha) => GIT_SHA.test(sha)) &&
      isText(body.summary) &&
      isStringArray(body.followUps)
    );
  }
  return (
    type === "mission.blocked" &&
    hasExactFields(body, [
      ["assignmentRefs", "summary", "blockers", "heldOn", "requiredAction"],
    ]) &&
    isReferenceArray(body.assignmentRefs, false) &&
    isText(body.summary) &&
    isStringArray(body.blockers, { nonempty: true }) &&
    isNullableText(body.heldOn) &&
    isText(body.requiredAction)
  );
}

function causalReferences(
  payload: CodingSessionTeamTransactionPayload,
): string[] {
  const body = payload.body as Record<string, unknown>;
  if (payload.type === "report") return [body.assignmentRef as string];
  if (payload.type === "verdict") {
    return [
      body.assignmentRef as string,
      body.reportRef as string,
      ...(body.refutationRef ? [body.refutationRef as string] : []),
    ];
  }
  if (payload.type === "acknowledgement") {
    return [body.acknowledgedEventRef as string];
  }
  if (payload.type.startsWith("mission.")) {
    return body.assignmentRefs as string[];
  }
  if (payload.type === "decision.answer") return [body.requestRef as string];
  // Neither a note's `refs` nor a request's `blocks` is causal: both are
  // pointers a reader follows, so either may name an event outside this set
  // and neither costs its record a place in the fold.
  return [];
}

/** Strict content decoder matching buzz-core's closed shapes and byte bounds. */
export function decodeCodingSessionTeamTransactionContent(
  source: string,
): StrictDecodeResult<CodingSessionTeamTransactionPayload> {
  if (byteLength(source) > 128 * 1024)
    return fail("content exceeds 131072 bytes");
  if (hasDuplicateJsonKeys(source))
    return fail("content has duplicate JSON keys");
  let value: unknown;
  try {
    value = JSON.parse(source);
  } catch {
    return fail("content is not valid JSON");
  }
  if (
    !hasExactFields(value, [
      [
        "schema",
        "sessionRef",
        "genesisRef",
        "type",
        "supersedes",
        "deliveryCommandId",
        "body",
      ],
    ]) ||
    value.schema !== CODING_SESSION_TEAM_TRANSACTION_SCHEMA ||
    typeof value.sessionRef !== "string" ||
    !UUID.test(value.sessionRef) ||
    typeof value.genesisRef !== "string" ||
    !HEX64.test(value.genesisRef) ||
    typeof value.type !== "string" ||
    !TYPES.has(value.type) ||
    !nullableReference(value.supersedes) ||
    !(
      value.deliveryCommandId === null ||
      (isText(value.deliveryCommandId, 256) &&
        ![...value.deliveryCommandId].some((character) =>
          /\p{Cc}/u.test(character),
        ))
    )
  ) {
    return fail("content does not match the v1 transaction schema");
  }
  const supersessionError = supersessionRefusal(
    value.type,
    value.supersedes,
    value.body,
  );
  if (supersessionError !== null) return fail(supersessionError);
  if (!validateBody(value.type, value.body)) {
    return fail("content does not match the v1 transaction schema");
  }
  return { ok: true, value: value as CodingSessionTeamTransactionPayload };
}

/** Verify signature, exact envelope, self-reference, and supplied session scope. */
export function decodeVerifiedCodingSessionTeamTransaction(input: {
  event: RelayEvent;
  channelRef: string;
  sessionRef: string;
  genesisRef: string;
}): StrictDecodeResult<VerifiedCodingSessionTeamTransaction> {
  const { event } = input;
  if (!hasValidSignature(event))
    return fail("transaction signature is invalid");
  if (event.kind !== KIND_CODING_SESSION_TEAM_TRANSACTION) {
    return fail("transaction kind is not 44244");
  }
  const decoded = decodeCodingSessionTeamTransactionContent(event.content);
  if (!decoded.ok) return decoded;
  const payload = decoded.value;
  const expectedTags = [
    ["h", input.channelRef],
    ["d", input.sessionRef],
    ["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA],
    ["cstx-genesis", input.genesisRef],
    ["cstx-type", payload.type],
  ];
  if (
    !UUID.test(input.channelRef) ||
    !UUID.test(input.sessionRef) ||
    !HEX64.test(input.genesisRef) ||
    JSON.stringify(event.tags) !== JSON.stringify(expectedTags) ||
    payload.sessionRef !== input.sessionRef ||
    payload.genesisRef !== input.genesisRef
  ) {
    return fail(
      "transaction envelope crosses or disagrees with its supplied scope",
    );
  }
  if (
    payload.supersedes === event.id ||
    causalReferences(payload).includes(event.id)
  ) {
    return fail("transaction cannot reference itself");
  }
  return {
    ok: true,
    value: {
      eventId: event.id,
      authorPubkey: event.pubkey,
      createdAt: event.created_at,
      payload,
      wireEvent: {
        id: event.id,
        pubkey: event.pubkey,
        created_at: event.created_at,
        kind: event.kind,
        tags: event.tags.map((tag) => [...tag]),
        content: event.content,
        sig: event.sig,
      },
    },
  };
}
