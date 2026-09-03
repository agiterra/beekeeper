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
  /** Every founder of the repository, lower-hex, signer first (finding 33). */
  readonly founders: readonly string[];
  /** Who may rewrite the rules, who the founders are, and whether this view
   * could read the project roster. Composed by `buzz-core`, never here. */
  readonly foundersNote: string;
  /** Whether the viewer's own key founds this repository. */
  readonly viewerIsFounder: boolean;
  /** The announcement's signer — the only key that may rewrite the rules in
   * v1 — or null when no announcement reached this view. */
  readonly rulesSigner: string | null;
  /** Whether the project roster half of the founder set was read. */
  readonly rosterRead: boolean;
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
        "founders",
        "foundersNote",
        "viewerIsFounder",
        "rulesSigner",
        "rosterRead",
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
    !Array.isArray(value.founders) ||
    !value.founders.every((founder) => typeof founder === "string") ||
    typeof value.foundersNote !== "string" ||
    typeof value.viewerIsFounder !== "boolean" ||
    (value.rulesSigner !== null && typeof value.rulesSigner !== "string") ||
    typeof value.rosterRead !== "boolean" ||
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
  /**
   * Pubkeys the repository's project roster grants Owner, or null when this
   * view could not read the roster.
   *
   * Finding 33: a repository's founders are its signer, its NIP-34
   * `maintainers`, and every Owner on the project roster its `project`
   * back-reference names. Null is "not read", never "there are none".
   */
  projectOwnerPubkeys: readonly string[] | null;
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
        projectOwnerPubkeys:
          input.projectOwnerPubkeys === null
            ? null
            : [...input.projectOwnerPubkeys],
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
 * Why {@link useCodingSessionMissionLand} answers `land: null` — finding 37.
 *
 * `"no-identity"` is not a failure: the surface has not yet resolved who is
 * asking (viewer/founder pubkey, session/genesis ref) or holds no fold
 * evidence yet, so there is nothing to land *yet*. `"boundary-failed"` is a
 * real fault: the native `coding_session_land` call threw, so the rule's
 * answer never reached this view at all. The panel's sentence must say which
 * — "nothing to land yet" and "the read failed" are different facts, and an
 * absent control that says neither is a comfortable silence.
 */
export type CodingSessionMissionLandUnavailableReason =
  | "no-identity"
  | "boundary-failed";

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
  /**
   * Who founds this repository, in every state — finding 33.
   *
   * Rendered whatever the answer is: "you are not one of this repository's
   * founders" is the single most useful thing a refused reader can be told,
   * and it is exactly what the pre-L18 screen could not say.
   */
  readonly foundersSentence: string;
  /** Whether the viewer's own key is one of them. */
  readonly viewerIsFounder: boolean;
  /**
   * Set, in every state, when the repository this rule ran against was not
   * the session's own `repoRef` but inferred from its project (LANE-L20 item
   * 2) — null when the repository (or its absence) is the session's own
   * signed fact.
   */
  readonly repositorySourceNote: string | null;
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

/**
 * The founder line, with names where the surface knows them.
 *
 * The sentence itself comes from `buzz-core` — one rule, one wording — and
 * this only prefixes who the viewer is relative to it. A key with no display
 * name renders as its first eight hex, the same shorthand every other line on
 * this surface uses.
 */
function foundersSentence(
  result: CodingSessionLandResult,
  resolveWho: (pubkey: string) => string,
): string {
  if (result.founders.length === 0) return result.foundersNote;
  const named = result.founders
    .map((founder) => resolveWho(founder))
    .join(", ");
  const you = result.viewerIsFounder
    ? "You are one of them."
    : "You are not one of them, so this repository's rules do not answer to your key.";
  const rules =
    result.rulesSigner === null
      ? ""
      : ` Rules are set by ${resolveWho(result.rulesSigner)} and only that key can rewrite them.`;
  // A roster this view could not read means the set may be missing a founder,
  // which is finding 33's own shape. Never silent.
  const roster = result.rosterRead
    ? ""
    : " The project roster was not read here, so an Owner on this repository's project is not listed.";
  return `Founders: ${named}. ${you}${rules}${roster}`;
}

/** Turn the native rule's answer into the sentences §1l freezes. */
export function codingSessionMissionLandModel(input: {
  result: CodingSessionLandResult;
  /** `{Who}` for a pubkey — the surface's own resolver. */
  resolveWho: (pubkey: string) => string;
  /**
   * Why no repository record reached the rule, when none did.
   *
   * Three different facts, and the sentence says which: the session's
   * creates named no repository at all (and its project, if any, names none
   * or more than one so nothing could be inferred either); they named one
   * whose kind:30617 this view did not read ("not read" is the honest word —
   * it is not "there is no rule"); or the session's project has two or more
   * repositories, so LANE-L20's read fallback could not pick one either.
   */
  repositoryUnknownReason?: "no-repo-ref" | "not-read" | "multiple-repos";
  /**
   * How many repositories the session's project has, when
   * `repositoryUnknownReason` is `"multiple-repos"`.
   */
  projectRepoCount?: number;
  /**
   * Whether the repository the native rule ran against was inferred from the
   * session's project (its only repository) rather than named by the
   * session's own `repoRef` — LANE-L20 item 2.
   */
  repositoryInferred?: boolean;
}): CodingSessionMissionLandModel {
  const { result } = input;
  const repositorySourceNote = input.repositoryInferred
    ? "This repository was inferred from the project's only repository."
    : null;
  const base = {
    buttonLabel: null,
    headSha: null,
    command: null,
    approvalSentence: null,
    notRunSentence: null,
    sentence: null,
    foundersSentence: foundersSentence(result, input.resolveWho),
    viewerIsFounder: result.viewerIsFounder,
    repositorySourceNote,
  } as const;

  if (!result.repositoryKnown) {
    return {
      ...base,
      state: "unknown",
      sentence:
        input.repositoryUnknownReason === "no-repo-ref"
          ? "This session's creates name no repository, so nothing here can say what gates a push."
          : input.repositoryUnknownReason === "multiple-repos"
            ? `This session's creates name no repository, and its project has ${input.projectRepoCount ?? 0} repositories, so nothing here can say which one gates a push.`
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
      foundersSentence: base.foundersSentence,
      viewerIsFounder: base.viewerIsFounder,
      repositorySourceNote: base.repositorySourceNote,
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
