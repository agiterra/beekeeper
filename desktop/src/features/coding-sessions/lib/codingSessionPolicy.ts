/**
 * The session policy the launch form collects — and nothing else.
 *
 * Every rule about what a kind:44245 record may contain lives in
 * `buzz-core::coding_session_policy` and reaches this file through the Tauri
 * boundary in `desktop/src-tauri/src/commands/coding_session_policy.rs`. This
 * module holds three things only:
 *
 * 1. the **draft** a person fills in, and how it becomes the content object
 *    the native builder is handed (an unset answer is an omitted key — never
 *    an explicit `null`, which the decoder refuses by name);
 * 2. an **exact-field decoder** of the adapter's own response, pinned in both
 *    directions to a fixture the adapter generates
 *    (`codingSessionPolicyAdapterResponse.fixture.json`);
 * 3. the sentences a surface owes a reader about a record nothing enforces.
 *
 * The four closed vocabularies are repeated here as *option lists* so the form
 * has something to offer. They are not a second implementation of the rule: a
 * word this file offers that `buzz-core` does not accept is refused at the
 * boundary, by name, before anything is signed — which is what
 * `every_refusal_is_the_core_decoders_own_sentence_naming_the_key` proves.
 */
import { invokeTauri } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";
import { hasExactFields } from "@/shared/coordination/sessionCoordinationStrictJson";

export const CODING_SESSION_POLICY_BUILD_COMMAND =
  "build_coding_session_policy_event";
export const CODING_SESSION_POLICY_READ_COMMAND =
  "decode_coding_session_policy_record";
export const CODING_SESSION_POLICY_BUILD_REQUEST_SCHEMA =
  "buzz-coding-session-policy-build-request/v1";
export const CODING_SESSION_POLICY_READ_REQUEST_SCHEMA =
  "buzz-coding-session-policy-read-request/v1";
export const CODING_SESSION_POLICY_ADAPTER_SCHEMA =
  "buzz-coding-session-policy-adapter/v1";

/** Options the form offers; `buzz-core` decides what it accepts. */
export const CODING_SESSION_POLICY_POSTURES = [
  "spike",
  "ship",
  "investigate",
  "overnight",
] as const;
/** Options the form offers; `buzz-core` decides what it accepts. */
export const CODING_SESSION_POLICY_ATTENTIONS = [
  "decisions",
  "decisions-and-milestones",
  "everything",
] as const;
/** Options the form offers; `buzz-core` decides what it accepts. */
export const CODING_SESSION_POLICY_CONTEXT_TIERS = [
  "standard",
  "long",
] as const;
/** Options the form offers; `buzz-core` decides what it accepts. */
export const CODING_SESSION_POLICY_IRREVERSIBLE_ACTS = [
  "push",
  "deploy",
  "delete",
  "external-message",
] as const;

export type CodingSessionPolicyPosture =
  (typeof CODING_SESSION_POLICY_POSTURES)[number];
export type CodingSessionPolicyAttention =
  (typeof CODING_SESSION_POLICY_ATTENTIONS)[number];
export type CodingSessionPolicyContextTier =
  (typeof CODING_SESSION_POLICY_CONTEXT_TIERS)[number];
export type CodingSessionPolicyIrreversibleAct =
  (typeof CODING_SESSION_POLICY_IRREVERSIBLE_ACTS)[number];

/**
 * What the launch form holds while a person is still answering.
 *
 * `null` and `[]` both mean "not set", and both become an *absent* key. The
 * withdrawal record — everything unset — is a legal 44245 and the only way to
 * take a policy back, so an empty draft is not an error here either.
 */
export type CodingSessionPolicyDraft = {
  posture: CodingSessionPolicyPosture | null;
  turns: number | null;
  tokensPerSeat: number | null;
  tokensPerSession: number | null;
  costUsdPerSession: number | null;
  contextTier: CodingSessionPolicyContextTier | null;
  attention: CodingSessionPolicyAttention | null;
  redFirst: boolean | null;
  reviewEveryLane: boolean | null;
  requiredGates: readonly string[];
  verifierRequired: boolean | null;
  benchIdentities: readonly string[];
  benchProviders: readonly string[];
  challengerSampleRate: number | null;
  irreversible: readonly CodingSessionPolicyIrreversibleAct[];
  timeBoxSecs: number | null;
  onMilestone: string | null;
};

/** An empty draft: a launch that sets no policy at all. */
export const EMPTY_CODING_SESSION_POLICY_DRAFT: CodingSessionPolicyDraft = {
  posture: null,
  turns: null,
  tokensPerSeat: null,
  tokensPerSession: null,
  costUsdPerSession: null,
  contextTier: null,
  attention: null,
  redFirst: null,
  reviewEveryLane: null,
  requiredGates: [],
  verifierRequired: null,
  benchIdentities: [],
  benchProviders: [],
  challengerSampleRate: null,
  irreversible: [],
  timeBoxSecs: null,
  onMilestone: null,
};

/** True when this draft would set at least one thing. */
export function codingSessionPolicyDraftSetsAnything(
  draft: CodingSessionPolicyDraft,
): boolean {
  const content = codingSessionPolicyContent({
    draft,
    sessionRef: "",
    genesisRef: "",
  });
  return Object.keys(content).length > 3;
}

function omitEmpty(
  entries: Record<string, unknown>,
): Record<string, unknown> | undefined {
  const kept = Object.fromEntries(
    Object.entries(entries).filter(([, value]) => {
      if (value === null || value === undefined) return false;
      return !(Array.isArray(value) && value.length === 0);
    }),
  );
  // An empty sub-object is refused by the decoder — the way to say "no budget"
  // is to omit the key — so a group nobody answered never appears at all.
  return Object.keys(kept).length > 0 ? kept : undefined;
}

/**
 * The NIP-CSP content object this draft stands for.
 *
 * Only the three required keys are ever unconditional. Everything else is
 * present exactly when it was answered, because "absent is not null" is the
 * rule the whole record is built on and a producer that writes `null` for an
 * unanswered question is refused by name.
 */
export function codingSessionPolicyContent(input: {
  draft: CodingSessionPolicyDraft;
  sessionRef: string;
  genesisRef: string;
}): Record<string, unknown> {
  const { draft } = input;
  const content: Record<string, unknown> = {
    schema: "buzz-coding-session-policy/v1",
    sessionRef: input.sessionRef,
    genesisRef: input.genesisRef,
  };
  if (draft.posture !== null) content.posture = draft.posture;
  const budget = omitEmpty({
    turns: draft.turns,
    tokensPerSeat: draft.tokensPerSeat,
    tokensPerSession: draft.tokensPerSession,
    costUsdPerSession: draft.costUsdPerSession,
    contextTier: draft.contextTier,
  });
  if (budget) content.budget = budget;
  if (draft.attention !== null) content.attention = draft.attention;
  const gates = omitEmpty({
    redFirst: draft.redFirst,
    reviewEveryLane: draft.reviewEveryLane,
    requiredGates: [...draft.requiredGates],
    verifierRequired: draft.verifierRequired,
  });
  if (gates) content.gates = gates;
  const bench = omitEmpty({
    identities: [...draft.benchIdentities],
    providers: [...draft.benchProviders],
    challengerSampleRate: draft.challengerSampleRate,
  });
  if (bench) content.bench = bench;
  if (draft.irreversible.length > 0) {
    content.irreversible = [...draft.irreversible];
  }
  const stop = omitEmpty({
    timeBoxSecs: draft.timeBoxSecs,
    onMilestone: draft.onMilestone,
  });
  if (stop) content.stop = stop;
  return content;
}

export type CodingSessionPolicyRecord = {
  readonly sessionRef: string;
  readonly genesisRef: string;
  readonly posture: string | null;
  readonly budget: {
    readonly turns: number | null;
    readonly tokensPerSeat: number | null;
    readonly tokensPerSession: number | null;
    readonly costUsdPerSession: number | null;
    readonly contextTier: string | null;
  } | null;
  readonly attention: string | null;
  readonly gates: {
    readonly redFirst: boolean | null;
    readonly reviewEveryLane: boolean | null;
    readonly requiredGates: readonly string[] | null;
    readonly verifierRequired: boolean | null;
  } | null;
  readonly bench: {
    readonly identities: readonly string[] | null;
    readonly providers: readonly string[] | null;
    readonly challengerSampleRate: number | null;
  } | null;
  readonly irreversible: readonly string[] | null;
  readonly stop: {
    readonly timeBoxSecs: number | null;
    readonly onMilestone: string | null;
  } | null;
  /** False exactly for the withdrawal record: a decision, never "unknown". */
  readonly setsAnyPolicy: boolean;
};

export type CodingSessionPolicyBuildResult = {
  readonly schema: typeof CODING_SESSION_POLICY_ADAPTER_SCHEMA;
  readonly implementation: "buzz-core";
  readonly kind: number;
  readonly content: string;
  readonly tags: readonly (readonly string[])[];
  readonly record: CodingSessionPolicyRecord;
};

export type CodingSessionPolicyReadResult = {
  readonly schema: typeof CODING_SESSION_POLICY_ADAPTER_SCHEMA;
  readonly implementation: "buzz-core";
  readonly eventId: string;
  readonly authorPubkey: string;
  readonly createdAt: number;
  readonly record: CodingSessionPolicyRecord;
};

function isNullableString(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

function isNullableNumber(value: unknown): value is number | null {
  return (
    value === null || (typeof value === "number" && Number.isFinite(value))
  );
}

function isNullableBoolean(value: unknown): value is boolean | null {
  return value === null || typeof value === "boolean";
}

function isNullableStringArray(value: unknown): value is string[] | null {
  return (
    value === null ||
    (Array.isArray(value) && value.every((item) => typeof item === "string"))
  );
}

function isBudget(
  value: unknown,
): value is CodingSessionPolicyRecord["budget"] {
  return (
    value === null ||
    (hasExactFields(value, [
      [
        "turns",
        "tokensPerSeat",
        "tokensPerSession",
        "costUsdPerSession",
        "contextTier",
      ],
    ]) &&
      isNullableNumber(value.turns) &&
      isNullableNumber(value.tokensPerSeat) &&
      isNullableNumber(value.tokensPerSession) &&
      isNullableNumber(value.costUsdPerSession) &&
      isNullableString(value.contextTier))
  );
}

function isGates(value: unknown): value is CodingSessionPolicyRecord["gates"] {
  return (
    value === null ||
    (hasExactFields(value, [
      ["redFirst", "reviewEveryLane", "requiredGates", "verifierRequired"],
    ]) &&
      isNullableBoolean(value.redFirst) &&
      isNullableBoolean(value.reviewEveryLane) &&
      isNullableStringArray(value.requiredGates) &&
      isNullableBoolean(value.verifierRequired))
  );
}

function isBench(value: unknown): value is CodingSessionPolicyRecord["bench"] {
  return (
    value === null ||
    (hasExactFields(value, [
      ["identities", "providers", "challengerSampleRate"],
    ]) &&
      isNullableStringArray(value.identities) &&
      isNullableStringArray(value.providers) &&
      isNullableNumber(value.challengerSampleRate))
  );
}

function isStop(value: unknown): value is CodingSessionPolicyRecord["stop"] {
  return (
    value === null ||
    (hasExactFields(value, [["timeBoxSecs", "onMilestone"]]) &&
      isNullableNumber(value.timeBoxSecs) &&
      isNullableString(value.onMilestone))
  );
}

/**
 * Decode one native policy record.
 *
 * Exported so a test can drive it with a fixture the Rust adapter generated.
 * A hand-written fixture is what let the team-fold decoder stay green while it
 * would have thrown for every real session (REVIEW-B1c B1), and the same trap
 * is open here: every key is required, so an adapter that stops disclosing one
 * is a loud failure rather than a field that silently reads as unset.
 */
export function decodeCodingSessionPolicyRecord(
  value: unknown,
): CodingSessionPolicyRecord {
  if (
    !hasExactFields(value, [
      [
        "sessionRef",
        "genesisRef",
        "posture",
        "budget",
        "attention",
        "gates",
        "bench",
        "irreversible",
        "stop",
        "setsAnyPolicy",
      ],
    ]) ||
    typeof value.sessionRef !== "string" ||
    typeof value.genesisRef !== "string" ||
    !isNullableString(value.posture) ||
    !isBudget(value.budget) ||
    !isNullableString(value.attention) ||
    !isGates(value.gates) ||
    !isBench(value.bench) ||
    !isNullableStringArray(value.irreversible) ||
    !isStop(value.stop) ||
    typeof value.setsAnyPolicy !== "boolean"
  ) {
    throw new Error("native session policy returned a malformed record");
  }
  return value as unknown as CodingSessionPolicyRecord;
}

/** Decode the build boundary's whole response. */
export function decodeCodingSessionPolicyBuildResult(
  value: unknown,
): CodingSessionPolicyBuildResult {
  if (
    !hasExactFields(value, [
      ["schema", "implementation", "kind", "content", "tags", "record"],
    ]) ||
    value.schema !== CODING_SESSION_POLICY_ADAPTER_SCHEMA ||
    value.implementation !== "buzz-core" ||
    !Number.isSafeInteger(value.kind) ||
    typeof value.content !== "string" ||
    !Array.isArray(value.tags) ||
    !value.tags.every(
      (tag) =>
        Array.isArray(tag) && tag.every((part) => typeof part === "string"),
    )
  ) {
    throw new Error(
      "native session policy returned a malformed build response",
    );
  }
  return {
    schema: CODING_SESSION_POLICY_ADAPTER_SCHEMA,
    implementation: "buzz-core",
    kind: value.kind as number,
    content: value.content,
    tags: value.tags as readonly (readonly string[])[],
    record: decodeCodingSessionPolicyRecord(value.record),
  };
}

/** Decode the read boundary's whole response. */
export function decodeCodingSessionPolicyReadResult(
  value: unknown,
): CodingSessionPolicyReadResult {
  if (
    !hasExactFields(value, [
      [
        "schema",
        "implementation",
        "eventId",
        "authorPubkey",
        "createdAt",
        "record",
      ],
    ]) ||
    value.schema !== CODING_SESSION_POLICY_ADAPTER_SCHEMA ||
    value.implementation !== "buzz-core" ||
    typeof value.eventId !== "string" ||
    typeof value.authorPubkey !== "string" ||
    !Number.isSafeInteger(value.createdAt)
  ) {
    throw new Error("native session policy returned a malformed read response");
  }
  return {
    schema: CODING_SESSION_POLICY_ADAPTER_SCHEMA,
    implementation: "buzz-core",
    eventId: value.eventId,
    authorPubkey: value.authorPubkey,
    createdAt: value.createdAt as number,
    record: decodeCodingSessionPolicyRecord(value.record),
  };
}

/**
 * Validate a draft and get back the exact unsigned kind:44245 event.
 *
 * The `content` that comes back is Rust's serialization, not this file's — so
 * what the keyring signs is what the decoder read, and a producer/consumer
 * disagreement cannot survive a single launch.
 */
export async function buildCodingSessionPolicyEvent(input: {
  channelRef: string;
  sessionRef: string;
  genesisRef: string;
  draft: CodingSessionPolicyDraft;
}): Promise<CodingSessionPolicyBuildResult> {
  return decodeCodingSessionPolicyBuildResult(
    await invokeTauri(CODING_SESSION_POLICY_BUILD_COMMAND, {
      request: {
        schema: CODING_SESSION_POLICY_BUILD_REQUEST_SCHEMA,
        channelRef: input.channelRef,
        policy: codingSessionPolicyContent({
          draft: input.draft,
          sessionRef: input.sessionRef,
          genesisRef: input.genesisRef,
        }),
      },
    }),
  );
}

/** Verify a signed kind:44245 event and read what it says. */
export async function readCodingSessionPolicyEvent(
  event: RelayEvent,
): Promise<CodingSessionPolicyReadResult> {
  return decodeCodingSessionPolicyReadResult(
    await invokeTauri(CODING_SESSION_POLICY_READ_COMMAND, {
      request: {
        schema: CODING_SESSION_POLICY_READ_REQUEST_SCHEMA,
        event,
      },
    }),
  );
}

/**
 * The sentence every surface rendering a policy owes its reader.
 *
 * `docs/design/portable-team-loop/POLICY.md` §4.2 — and it changed under this
 * constant while both halves of batch 2 were in flight. Lane B3 wrote it when
 * §4 still said *"until a consumer exists"* and nothing anywhere refused a
 * turn on a 44245; lane B2 then shipped the first consumer, so that wording
 * became false in the one way that matters — it told a founder nothing was
 * counting their turn ceiling while the provider was refusing turns on it.
 *
 * The sentence is POLICY.md §4.2's own, byte-for-byte, and is the same string
 * `bee sessions policy get` and the Tauri adapter return: **every** enforced
 * field is named, and every other field is *"read and shown, never counted"*.
 * {@link CODING_SESSION_POLICY_ENFORCED_FIELDS} marks those rows. Kept as one
 * constant so no surface can drift into softer wording — and held to the
 * others by `crates/buzz-cli/tests/policy_enforcement_sentence.rs`, after
 * REVIEW-L7 F1 found six copies claiming one enforced field when there were
 * two.
 */
export const CODING_SESSION_POLICY_STATED_NOT_ENFORCED =
  "Enforced: budget.turns at the provider's turn gate, and " +
  "gates.verifierRequired at the fold's completion check. Every other field " +
  "is read and shown, never counted.";

/**
 * Policy fields a consumer actually enforces today, by dotted name.
 *
 * The *only* place that claim is made. `budget.turns` is here because lane
 * B2.4 shipped its consumer: `exhausted_umbrella_budget`
 * (`crates/buzz-session-provider/src/commands.rs`), reached from the 44220
 * turn gate and from a create's first turn, which override
 * `BUZZ_CSP_TURN_BUDGET` for that umbrella. The umbrella's founder is still
 * never refused, and a policy published while a seat is already running does
 * not bind until that umbrella's next create or resume — POLICY.md §4.1.
 *
 * `gates.verifierRequired` joined it on 2026-09-02 (batch 3, item G): its
 * consumer is the 44244 fold's completion check
 * (`crates/buzz-core/src/coding_session_completion_verification.rs`), which
 * excludes a `mission.completed` whose settled assignments carry no active
 * verifier's ruling, and which `bee sessions complete` refuses to sign past.
 * Desktop's own fold passes `verifierRequired: false` until the policy hook
 * reaches it, so this name says the *repository* counts the field, not that
 * every surface does.
 *
 * Every other field stays out until the same thing is true of it. Adding a
 * name here with no consumer behind it is the exact lie this list exists to
 * prevent.
 */
export const CODING_SESSION_POLICY_ENFORCED_FIELDS: readonly string[] = [
  "budget.turns",
  "gates.verifierRequired",
];

/** True when nothing in this build consumes any field this record sets. */
export function codingSessionPolicyIsEnforced(field: string): boolean {
  return CODING_SESSION_POLICY_ENFORCED_FIELDS.includes(field);
}

/** One rendered row: a dotted field name, its value as words, its standing. */
export type CodingSessionPolicyFact = {
  /** Dotted field name, matching NIP-CSP exactly. */
  field: string;
  /** The heading a person reads. */
  label: string;
  /** The value, already in words. */
  value: string;
  /** True only when something in this build reads the field. */
  enforced: boolean;
};

/**
 * A decoded policy as rows a surface can render without interpreting it.
 *
 * Only fields the record actually sets appear. An unset field is not rendered
 * as `—`: this is a *statement of intent* and an empty row would read as "the
 * founder said nothing about pushes" in exactly the place where a reader is
 * deciding what a team may do.
 */
export function codingSessionPolicyFacts(
  record: CodingSessionPolicyRecord,
): CodingSessionPolicyFact[] {
  const facts: CodingSessionPolicyFact[] = [];
  const push = (field: string, label: string, value: string | null) => {
    if (value === null) return;
    facts.push({
      field,
      label,
      value,
      enforced: codingSessionPolicyIsEnforced(field),
    });
  };
  const count = (value: number | null, unit: string) =>
    value === null ? null : `${value.toLocaleString()} ${unit}`;

  push("posture", "Posture", record.posture);
  push("budget.turns", "Turns", count(record.budget?.turns ?? null, "turns"));
  push(
    "budget.tokensPerSeat",
    "Tokens per seat",
    count(record.budget?.tokensPerSeat ?? null, "tokens"),
  );
  push(
    "budget.tokensPerSession",
    "Tokens per session",
    count(record.budget?.tokensPerSession ?? null, "tokens"),
  );
  push(
    "budget.costUsdPerSession",
    "Cost per session",
    record.budget?.costUsdPerSession == null
      ? null
      : `$${record.budget.costUsdPerSession.toLocaleString()}`,
  );
  push("budget.contextTier", "Context", record.budget?.contextTier ?? null);
  push("attention", "Tell me about", record.attention);
  push(
    "gates.redFirst",
    "Red first",
    record.gates?.redFirst == null
      ? null
      : record.gates.redFirst
        ? "yes"
        : "no",
  );
  push(
    "gates.reviewEveryLane",
    "Review every lane",
    record.gates?.reviewEveryLane == null
      ? null
      : record.gates.reviewEveryLane
        ? "yes"
        : "no",
  );
  push(
    "gates.requiredGates",
    "Required gates",
    record.gates?.requiredGates?.join(" · ") ?? null,
  );
  push(
    "gates.verifierRequired",
    "Verifier required",
    record.gates?.verifierRequired == null
      ? null
      : record.gates.verifierRequired
        ? "yes"
        : "no",
  );
  push(
    "bench.identities",
    "Bench identities",
    record.bench?.identities
      ? count(record.bench.identities.length, "identities")
      : null,
  );
  push(
    "bench.providers",
    "Bench providers",
    record.bench?.providers?.join(" · ") ?? null,
  );
  push(
    "bench.challengerSampleRate",
    "Challenger rate",
    record.bench?.challengerSampleRate == null
      ? null
      : `${Math.round(record.bench.challengerSampleRate * 100)}%`,
  );
  push(
    "irreversible",
    "Needs your word",
    record.irreversible?.join(" · ") ?? null,
  );
  push(
    "stop.timeBoxSecs",
    "Time box",
    record.stop?.timeBoxSecs == null
      ? null
      : `${Math.round(record.stop.timeBoxSecs / 3600).toLocaleString()} h`,
  );
  push("stop.onMilestone", "Stop on", record.stop?.onMilestone ?? null);
  return facts;
}
