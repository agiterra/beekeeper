/**
 * Publishing a kind:44244 the founder signed in the app.
 *
 * Live run 2 ended with the founder answering three rulings from a terminal
 * while the app that *showed* the question could not take the answer. This is
 * the path that closes it, and it is deliberately the same one the launch uses
 * for the 44245 policy: a Rust command builds the exact unsigned event, the
 * desktop keyring signs **Rust's own serialization**, and the relay client
 * publishes it. TypeScript never serializes a 44244 body — the content string
 * that gets signed is the one `buzz-core`'s decoder read — so a producer and a
 * consumer that disagree about those bytes cannot survive one answer (I6).
 *
 * The one thing this file does own is the *draft object* a form fills in, and
 * even that is handed straight to the native decoder: an unset answer is an
 * omitted key, never an explicit `null`, and every bound belongs to the crate
 * that refuses it. The bounds a counter shows come back from
 * {@link codingSessionTeamTransactionCapabilities} rather than being spelled
 * here.
 */
import { invokeTauri, signRelayEvent } from "@/shared/api/tauri";
import { relayClient } from "@/shared/api/relayClient";
import { hasExactFields } from "@/shared/coordination/sessionCoordinationStrictJson";

/** Tauri command that turns a draft transaction into unsigned bytes. */
export const CODING_SESSION_TEAM_TRANSACTION_BUILD_COMMAND =
  "build_coding_session_team_transaction_event";
/** Tauri command that reports which optional keys this build's core accepts. */
export const CODING_SESSION_TEAM_TRANSACTION_CAPABILITIES_COMMAND =
  "coding_session_team_transaction_capabilities";
/** Closed request schema the build boundary accepts. */
export const CODING_SESSION_TEAM_TRANSACTION_BUILD_REQUEST_SCHEMA =
  "buzz-coding-session-team-transaction-build-request/v1";
/** Closed schema the native adapter answers with. */
export const CODING_SESSION_TEAM_TRANSACTION_ADAPTER_SCHEMA =
  "buzz-coding-session-team-transaction-adapter/v1";

/**
 * Which optional `decision.answer` keys this build's `buzz-core` accepts, and
 * the bounds it enforces.
 *
 * `supportsDecisionAnswerCondition` is **measured** on the Rust side by running
 * the real decoder over a canonical answer carrying the key — not read from a
 * version number. Until lane L7's `condition` lands in `buzz-core` it is
 * `false`, the form does not offer the field, and a surface says so rather
 * than showing an input whose value the relay would refuse.
 */
export type CodingSessionTeamTransactionCapabilities = {
  readonly schema: typeof CODING_SESSION_TEAM_TRANSACTION_ADAPTER_SCHEMA;
  readonly implementation: "buzz-core";
  readonly choiceMaxBytes: number;
  readonly noteMaxBytes: number;
  readonly conditionMaxBytes: number;
  readonly supportsDecisionAnswerCondition: boolean;
};

/** The unsigned kind:44244 event, plus what its bytes say. */
export type CodingSessionTeamTransactionBuildResult = {
  readonly schema: typeof CODING_SESSION_TEAM_TRANSACTION_ADAPTER_SCHEMA;
  readonly implementation: "buzz-core";
  readonly kind: number;
  readonly content: string;
  readonly tags: readonly (readonly string[])[];
  readonly record: {
    readonly sessionRef: string;
    readonly genesisRef: string;
    readonly type: string;
    readonly body: Record<string, unknown>;
  };
};

/**
 * Decode the capability boundary's whole response.
 *
 * Exact fields in both directions, pinned by
 * `codingSessionTeamTransactionCapabilities.fixture.json` — the adapter's own
 * output. A hand-written fixture is what let the team-fold decoder stay green
 * while it would have thrown for every real session (REVIEW-B1c B1).
 */
export function decodeCodingSessionTeamTransactionCapabilities(
  value: unknown,
): CodingSessionTeamTransactionCapabilities {
  if (
    !hasExactFields(value, [
      [
        "schema",
        "implementation",
        "choiceMaxBytes",
        "noteMaxBytes",
        "conditionMaxBytes",
        "supportsDecisionAnswerCondition",
      ],
    ]) ||
    value.schema !== CODING_SESSION_TEAM_TRANSACTION_ADAPTER_SCHEMA ||
    value.implementation !== "buzz-core" ||
    !Number.isSafeInteger(value.choiceMaxBytes) ||
    !Number.isSafeInteger(value.noteMaxBytes) ||
    !Number.isSafeInteger(value.conditionMaxBytes) ||
    typeof value.supportsDecisionAnswerCondition !== "boolean"
  ) {
    throw new Error(
      "native team-transaction adapter returned malformed capabilities",
    );
  }
  return value as unknown as CodingSessionTeamTransactionCapabilities;
}

/** Decode the build boundary's whole response. */
export function decodeCodingSessionTeamTransactionBuildResult(
  value: unknown,
): CodingSessionTeamTransactionBuildResult {
  if (
    !hasExactFields(value, [
      ["schema", "implementation", "kind", "content", "tags", "record"],
    ]) ||
    value.schema !== CODING_SESSION_TEAM_TRANSACTION_ADAPTER_SCHEMA ||
    value.implementation !== "buzz-core" ||
    !Number.isSafeInteger(value.kind) ||
    typeof value.content !== "string" ||
    !Array.isArray(value.tags) ||
    !value.tags.every(
      (tag) =>
        Array.isArray(tag) && tag.every((part) => typeof part === "string"),
    ) ||
    !hasExactFields(value.record, [
      ["sessionRef", "genesisRef", "type", "body"],
    ]) ||
    typeof value.record.sessionRef !== "string" ||
    typeof value.record.genesisRef !== "string" ||
    typeof value.record.type !== "string" ||
    typeof value.record.body !== "object" ||
    value.record.body === null
  ) {
    throw new Error(
      "native team-transaction adapter returned a malformed build response",
    );
  }
  return value as unknown as CodingSessionTeamTransactionBuildResult;
}

/** Ask this build's core which optional keys and bounds a form may offer. */
export async function codingSessionTeamTransactionCapabilities(): Promise<CodingSessionTeamTransactionCapabilities> {
  return decodeCodingSessionTeamTransactionCapabilities(
    await invokeTauri(CODING_SESSION_TEAM_TRANSACTION_CAPABILITIES_COMMAND, {}),
  );
}

/** What a founder answering a ruling has filled in. */
export type CodingSessionDecisionAnswerDraft = {
  /** Event id of the `decision.request` being answered. */
  readonly requestRef: string;
  /**
   * The chosen option's index, or the words a free-text answer used.
   *
   * A number can only be an index and a string can only be text — the wire's
   * own untagged shape, so the two never collide.
   */
  readonly choice: number | string;
  /** Optional reasoning; an unanswered field is `null`, never `""`. */
  readonly note: string | null;
  /**
   * Optional §1k condition: the class this ruling covers.
   *
   * Sent only when {@link CodingSessionTeamTransactionCapabilities} says this
   * build's core accepts the key. Where it does not, the key is **omitted**
   * (absent, not null) and the caller is told rather than silently dropping
   * what a person typed.
   */
  readonly condition: string | null;
};

/**
 * The NIP-CSTX content object this draft stands for.
 *
 * Exported so a test can assert the exact keys handed to the native builder
 * without going through Tauri. It carries no bound and no vocabulary of its
 * own: `buzz-core` refuses anything it should, by name, before signing.
 */
export function codingSessionDecisionAnswerContent(input: {
  draft: CodingSessionDecisionAnswerDraft;
  sessionRef: string;
  genesisRef: string;
  supportsCondition: boolean;
}): Record<string, unknown> {
  const body: Record<string, unknown> = {
    requestRef: input.draft.requestRef,
    choice: input.draft.choice,
    note: input.draft.note,
  };
  // Present-as-null when the key exists on the wire, absent when it does not.
  // Those are different bodies and core's exact-key decoder refuses the wrong
  // one, which is exactly why the answer comes from a capability probe.
  if (input.supportsCondition) body.condition = input.draft.condition;
  return {
    schema: "buzz-coding-session-team-transaction/v1",
    sessionRef: input.sessionRef,
    genesisRef: input.genesisRef,
    type: "decision.answer",
    supersedes: null,
    deliveryCommandId: null,
    body,
  };
}

/**
 * Validate a draft transaction and get back the exact unsigned kind:44244.
 *
 * The `content` that comes back is Rust's serialization, not this file's.
 */
export async function buildCodingSessionTeamTransactionEvent(input: {
  channelRef: string;
  transaction: Record<string, unknown>;
}): Promise<CodingSessionTeamTransactionBuildResult> {
  return decodeCodingSessionTeamTransactionBuildResult(
    await invokeTauri(CODING_SESSION_TEAM_TRANSACTION_BUILD_COMMAND, {
      request: {
        schema: CODING_SESSION_TEAM_TRANSACTION_BUILD_REQUEST_SCHEMA,
        channelRef: input.channelRef,
        transaction: input.transaction,
      },
    }),
  );
}

/** Everything outside this module that publishing an answer has to reach. */
export type CodingSessionDecisionAnswerDeps = {
  buildEvent: typeof buildCodingSessionTeamTransactionEvent;
  capabilities: typeof codingSessionTeamTransactionCapabilities;
  signer: typeof signRelayEvent;
  publisher: { publishEvent: typeof relayClient.publishEvent };
};

/** The real thing: this computer's Tauri boundary, keyring and relay. */
export const DEFAULT_CODING_SESSION_DECISION_ANSWER_DEPS: CodingSessionDecisionAnswerDeps =
  {
    buildEvent: buildCodingSessionTeamTransactionEvent,
    capabilities: codingSessionTeamTransactionCapabilities,
    signer: signRelayEvent,
    publisher: relayClient,
  };

/** What one published answer left on the wire. */
export type CodingSessionDecisionAnswerPublished = {
  readonly eventId: string;
  /** True when the published body actually carried §1k's `condition`. */
  readonly conditionSent: boolean;
};

/**
 * Build, sign and publish one `decision.answer`.
 *
 * Nothing here decides whether the row is now answered: that comes from the
 * next fold. A queue that showed "answered" because a publish returned would
 * be the same lie as a badge with no event behind it.
 */
export async function publishCodingSessionDecisionAnswer(input: {
  channelRef: string;
  sessionRef: string;
  genesisRef: string;
  draft: CodingSessionDecisionAnswerDraft;
  deps?: CodingSessionDecisionAnswerDeps;
}): Promise<CodingSessionDecisionAnswerPublished> {
  const deps = input.deps ?? DEFAULT_CODING_SESSION_DECISION_ANSWER_DEPS;
  const capabilities = await deps.capabilities();
  if (
    input.draft.condition !== null &&
    !capabilities.supportsDecisionAnswerCondition
  ) {
    throw new Error(
      "This build cannot carry a condition on an answer yet, so the ruling was not published. Remove the condition, or update Beekeeper and the relay first.",
    );
  }
  const transaction = codingSessionDecisionAnswerContent({
    draft: input.draft,
    sessionRef: input.sessionRef,
    genesisRef: input.genesisRef,
    supportsCondition: capabilities.supportsDecisionAnswerCondition,
  });
  const built = await deps.buildEvent({
    channelRef: input.channelRef,
    transaction,
  });
  // What is signed is Rust's own serialization of the record, never this
  // side's: a producer and a reader that disagree about the bytes cannot
  // survive one answer.
  const event = await deps.signer({
    kind: built.kind,
    content: built.content,
    tags: built.tags.map((tag) => [...tag]),
  });
  const accepted = await deps.publisher.publishEvent(
    event,
    "Timed out while publishing the answer.",
    "Failed to publish the answer.",
  );
  return {
    eventId: accepted.id,
    conditionSent:
      capabilities.supportsDecisionAnswerCondition &&
      input.draft.condition !== null,
  };
}
