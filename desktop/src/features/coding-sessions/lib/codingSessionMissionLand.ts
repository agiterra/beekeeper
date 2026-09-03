/**
 * Whether this mission's commit may land on `main` — and the command a person
 * runs to land it.
 *
 * Finding 27: a verifier's FAIL was on the wire and the branch landed on `main`
 * anyway, because nothing in the push path had ever read a verdict. Lane L6
 * makes the relay's pre-receive hook read it; this file makes the founder's own
 * screen read **the same rule from the same Rust function**, through the
 * `coding_session_land` boundary. TypeScript re-implements none of it (I6):
 * every answer below — admitted, refused, and the refusal's exact words — comes
 * back from that command.
 *
 * **The app never runs the push.** That is a ruling (§1l, LANE-L8 addendum 4),
 * and the three reasons are in the copy a person actually reads: the push is
 * irreversible, this app holds no checkout, and the git credential lives in the
 * founder's shell. What this surface offers is the exact command and a Copy
 * button.
 */
import { invokeTauri } from "@/shared/api/tauri";
import { hasExactFields } from "@/shared/coordination/sessionCoordinationStrictJson";
import type { ImmutableCodingSessionTeamWireEvent } from "./invokeCodingSessionTeamFold";

/** Tauri command that asks the push path's own rule. */
export const CODING_SESSION_LAND_COMMAND = "coding_session_land";
/** Closed request schema that boundary accepts. */
export const CODING_SESSION_LAND_REQUEST_SCHEMA =
  "buzz-coding-session-land-request/v1";
/** Closed schema the native adapter answers with. */
export const CODING_SESSION_LAND_ADAPTER_SCHEMA =
  "buzz-coding-session-land-adapter/v1";

/** The ref a mission lands on. Frozen in §1l alongside the command. */
export const CODING_SESSION_LAND_REF = "refs/heads/main";

/** What admitted a commit. */
export type CodingSessionLandEvidence = {
  readonly sessionRef: string;
  readonly dispositionEventId: string;
  readonly dispositionAuthorPubkey: string;
  readonly reportEventId: string;
  readonly headSha: string;
};

/** The newest canonical verdict the mission holds, admitting or not. */
export type CodingSessionLandNewestVerdict = {
  readonly eventId: string;
  readonly authorPubkey: string;
  readonly decision: string;
  readonly reportEventId: string;
  readonly headSha: string | null;
};

/** The native rule's whole answer for one mission and one ref. */
export type CodingSessionLandResult = {
  readonly schema: typeof CODING_SESSION_LAND_ADAPTER_SCHEMA;
  readonly implementation: "buzz-core";
  readonly repositoryKnown: boolean;
  readonly ruleGoverns: boolean;
  readonly admitted: boolean;
  readonly evidence: CodingSessionLandEvidence | null;
  readonly refusalReason: string | null;
  readonly newestVerdict: CodingSessionLandNewestVerdict | null;
  readonly command: string | null;
};

function isEvidence(value: unknown): value is CodingSessionLandEvidence | null {
  return (
    value === null ||
    (hasExactFields(value, [
      [
        "sessionRef",
        "dispositionEventId",
        "dispositionAuthorPubkey",
        "reportEventId",
        "headSha",
      ],
    ]) &&
      typeof value.sessionRef === "string" &&
      typeof value.dispositionEventId === "string" &&
      typeof value.dispositionAuthorPubkey === "string" &&
      typeof value.reportEventId === "string" &&
      typeof value.headSha === "string")
  );
}

function isNewestVerdict(
  value: unknown,
): value is CodingSessionLandNewestVerdict | null {
  return (
    value === null ||
    (hasExactFields(value, [
      ["eventId", "authorPubkey", "decision", "reportEventId", "headSha"],
    ]) &&
      typeof value.eventId === "string" &&
      typeof value.authorPubkey === "string" &&
      typeof value.decision === "string" &&
      typeof value.reportEventId === "string" &&
      (value.headSha === null || typeof value.headSha === "string"))
  );
}

/**
 * Decode the boundary's whole answer, every key required.
 *
 * Pinned to `codingSessionLandAdapterResponse.fixture.json`, which the Rust
 * adapter generates. An adapter that stops disclosing a field is a loud
 * failure rather than one that quietly reads as `false`.
 */
export function decodeCodingSessionLandResult(
  value: unknown,
): CodingSessionLandResult {
  if (
    !hasExactFields(value, [
      [
        "schema",
        "implementation",
        "repositoryKnown",
        "ruleGoverns",
        "admitted",
        "evidence",
        "refusalReason",
        "newestVerdict",
        "command",
      ],
    ]) ||
    value.schema !== CODING_SESSION_LAND_ADAPTER_SCHEMA ||
    value.implementation !== "buzz-core" ||
    typeof value.repositoryKnown !== "boolean" ||
    typeof value.ruleGoverns !== "boolean" ||
    typeof value.admitted !== "boolean" ||
    !isEvidence(value.evidence) ||
    (value.refusalReason !== null && typeof value.refusalReason !== "string") ||
    !isNewestVerdict(value.newestVerdict) ||
    (value.command !== null && typeof value.command !== "string")
  ) {
    throw new Error("native land adapter returned a malformed response");
  }
  return value as unknown as CodingSessionLandResult;
}

/** Ask the push path's rule whether this mission's commit may land. */
export async function invokeCodingSessionLand(input: {
  refName?: string;
  sessionRef: string;
  genesisRef: string;
  founderPubkey: string;
  repoOwnerPubkey: string | null;
  pusherPubkey: string;
  protectionTags: readonly (readonly string[])[] | null;
  includedEventIds: readonly string[];
  events: readonly ImmutableCodingSessionTeamWireEvent[];
}): Promise<CodingSessionLandResult> {
  return decodeCodingSessionLandResult(
    await invokeTauri(CODING_SESSION_LAND_COMMAND, {
      request: {
        schema: CODING_SESSION_LAND_REQUEST_SCHEMA,
        refName: input.refName ?? CODING_SESSION_LAND_REF,
        sessionRef: input.sessionRef,
        genesisRef: input.genesisRef,
        founderPubkey: input.founderPubkey,
        repoOwnerPubkey: input.repoOwnerPubkey,
        pusherPubkey: input.pusherPubkey,
        protectionTags:
          input.protectionTags === null
            ? null
            : input.protectionTags.map((tag) => [...tag]),
        includedEventIds: [...input.includedEventIds],
        events: input.events.map((event) => ({
          id: event.id,
          pubkey: event.pubkey,
          created_at: event.created_at,
          kind: event.kind,
          tags: event.tags.map((tag) => [...tag]),
          content: event.content,
          sig: event.sig,
        })),
      },
    }),
  );
}

/** First eight hex of an event id — §1f's `{8hex}`. */
function short(eventId: string): string {
  return eventId.slice(0, 8);
}

/**
 * What the Land control says, in §1l's own words.
 *
 * Four states, and the control is **present in every one of them**: a missing
 * control would leave a founder guessing which of "not approved", "not read"
 * and "not governed" they were looking at.
 */
export type CodingSessionMissionLandModel = {
  /** `ready` alone enables the control; every other state still renders. */
  readonly state: "ready" | "refused" | "ungoverned" | "unknown";
  /** `Land {sha7} on main`, or null when nothing is offered. */
  readonly buttonLabel: string | null;
  /** The commit, verbatim, or null. */
  readonly headSha: string | null;
  /** The exact command, or null. Names the commit, never the branch. */
  readonly command: string | null;
  /** The confirm step's first sentence, or null. */
  readonly approvalSentence: string | null;
  /** The confirm step's second sentence — why the app does not run it. */
  readonly notRunSentence: string | null;
  /** `Not ready to land: ` + §1j verbatim, or the ungoverned/unknown line. */
  readonly sentence: string | null;
};

/**
 * §1l's second confirm sentence, frozen.
 *
 * The three reasons are in the copy rather than in a comment because the next
 * person to ask "why doesn't this just push?" is a reader of the screen, not a
 * reader of this file.
 */
export const CODING_SESSION_LAND_NOT_RUN_SENTENCE =
  "Beekeeper does not run this for you: the push is irreversible, this app " +
  "holds no checkout, and your git credential lives in your shell. Run it " +
  "there:";

/** Turn the native rule's answer into the sentences §1l freezes. */
export function codingSessionMissionLandModel(input: {
  result: CodingSessionLandResult;
  /** `{Who}` for a pubkey — the surface's own resolver. */
  resolveWho: (pubkey: string) => string;
  /**
   * Why no repository record reached the rule, when none did.
   *
   * Two different facts, and the sentence says which: the session's creates
   * named no repository at all, or they named one whose kind:30617 this view
   * did not read. "Not read" is the honest word for the second — it is not
   * "there is no rule".
   */
  repositoryUnknownReason?: "no-repo-ref" | "not-read";
}): CodingSessionMissionLandModel {
  const { result } = input;
  const base = {
    buttonLabel: null,
    headSha: null,
    command: null,
    approvalSentence: null,
    notRunSentence: null,
    sentence: null,
  } as const;

  if (!result.repositoryKnown) {
    return {
      ...base,
      state: "unknown",
      sentence:
        input.repositoryUnknownReason === "no-repo-ref"
          ? "This session's creates name no repository, so nothing here can say what gates a push."
          : "The repository this session names was not read here, so nothing can say what gates a push.",
    };
  }

  if (!result.ruleGoverns) {
    const verdict = result.newestVerdict;
    return {
      ...base,
      state: "ungoverned",
      sentence:
        verdict === null
          ? "This repository has no require-verdict rule, so nothing here gates the push. This mission holds no verdict yet."
          : `This repository has no require-verdict rule, so nothing here gates the push. The mission's newest verdict is ${verdict.decision} by ${input.resolveWho(verdict.authorPubkey)} over report ${short(verdict.reportEventId)}.`,
    };
  }

  if (result.admitted && result.evidence !== null && result.command !== null) {
    const evidence = result.evidence;
    return {
      state: "ready",
      buttonLabel: `Land ${evidence.headSha.slice(0, 7)} on main`,
      headSha: evidence.headSha,
      command: result.command,
      approvalSentence: `Approved by ${input.resolveWho(evidence.dispositionAuthorPubkey)} in disposition ${short(evidence.dispositionEventId)}, over report ${short(evidence.reportEventId)}. The relay's require-verdict rule admits this commit on ${CODING_SESSION_LAND_REF}.`,
      notRunSentence: CODING_SESSION_LAND_NOT_RUN_SENTENCE,
      sentence: null,
    };
  }

  return {
    ...base,
    state: "refused",
    // §1j's string verbatim behind §1l's prefix. A paraphrase would drift from
    // the words the relay will actually print at push time.
    sentence: `Not ready to land: ${result.refusalReason ?? "the rule refused this commit and gave no reason, which is itself a defect worth reporting."}`,
  };
}
