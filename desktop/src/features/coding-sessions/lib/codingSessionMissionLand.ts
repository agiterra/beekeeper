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

/** Which arm of the require-verdict rule admitted a commit. */
export type CodingSessionLandArm =
  | "founder"
  | "observed-gates"
  | "verifier-verdict";

/**
 * What admitted a commit.
 *
 * Under arm `founder` every field but `arm` and `headSha` is empty: a
 * founder's push reads no mission, so there is no disposition, no report and
 * no verifier to name. The screen must say *why* it is ready — "you are a
 * founder" and "a machine checked it" are different facts, and one sentence
 * covering both would be the comfortable guess.
 */
export type CodingSessionLandEvidence = {
  readonly arm: CodingSessionLandArm;
  readonly sessionRef: string;
  readonly dispositionEventId: string;
  readonly dispositionAuthorPubkey: string;
  readonly refutationEventId: string;
  readonly verifierPubkey: string;
  /**
   * The gates that had to be green on this commit, in the order they were
   * required. Empty only under arm `founder`, which reads no mission.
   *
   * Under `verifier-verdict` too since the 2026-09-03 follow-up ruling: that
   * arm is the clearance **and** these rows, and a confirm step naming only
   * the verifier would say half of what admitted the push.
   */
  readonly observedGates: readonly string[];
  readonly reportEventId: string;
  readonly headSha: string;
  /**
   * How the mission's kind 44245 policy resolved under arms (B) and (C):
   * `present`, `withdrawn` or `absent` (finding 89). Empty under arm
   * `founder`, which reads no policy at all.
   *
   * "Nobody set a policy" and "the founder withdrew it" are different facts,
   * and both apply the default gate list — a screen that showed only the
   * gates could not tell them apart.
   */
  readonly policyResolution: string;
  /** The kind 44245 record the arm read, or null when none named the mission. */
  readonly policyEventId: string | null;
  /**
   * Why no policy was consulted, under arm `founder` only:
   * `founder_exception`.
   *
   * The 2026-09-05 audit asked for exactly this: a founder's landing is the
   * deliberate exception and must never read as verifier-approved.
   */
  readonly policyNotEvaluated: string | null;
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
  /**
   * Whether the mission's seat roster reached the rule.
   *
   * `false` means arm (C) could not be evaluated from this surface at all —
   * never that no verifier cleared the report. The refusal sentence is then
   * about a check that did not run, and the screen says so.
   */
  readonly seatsRead: boolean;
  /**
   * Whether any folded kind 44246 gate row reached the rule.
   *
   * Half of arm (C) since the 2026-09-03 follow-up ruling, so `false` means
   * the gate half could not be evaluated from this surface — never that the
   * gates were not green. A refusal naming a gate with "no observed green row"
   * is then a fact about this screen's reads, and it says so.
   */
  readonly gateRowsRead: boolean;
  /**
   * Whether the mission's repository binding reached the rule (finding 91).
   *
   * `false` means this answer **assumed** the mission is bound to the
   * repository on screen. The relay checks the real binding, so a `false`
   * here is the one place this surface can be friendlier than the gate.
   */
  readonly boundRepositoriesRead: boolean;
  readonly command: string | null;
};

function isEvidence(value: unknown): value is CodingSessionLandEvidence | null {
  return (
    value === null ||
    (hasExactFields(value, [
      [
        "arm",
        "sessionRef",
        "dispositionEventId",
        "dispositionAuthorPubkey",
        "refutationEventId",
        "verifierPubkey",
        "reportEventId",
        "headSha",
        "observedGates",
        "policyResolution",
        "policyEventId",
        "policyNotEvaluated",
      ],
    ]) &&
      (value.arm === "founder" ||
        value.arm === "observed-gates" ||
        value.arm === "verifier-verdict") &&
      Array.isArray(value.observedGates) &&
      value.observedGates.every((gate) => typeof gate === "string") &&
      typeof value.sessionRef === "string" &&
      typeof value.dispositionEventId === "string" &&
      typeof value.dispositionAuthorPubkey === "string" &&
      typeof value.refutationEventId === "string" &&
      typeof value.verifierPubkey === "string" &&
      typeof value.reportEventId === "string" &&
      typeof value.headSha === "string" &&
      typeof value.policyResolution === "string" &&
      (value.policyEventId === null ||
        typeof value.policyEventId === "string") &&
      (value.policyNotEvaluated === null ||
        typeof value.policyNotEvaluated === "string"))
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
        "seatsRead",
        "gateRowsRead",
        "boundRepositoriesRead",
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
    typeof value.seatsRead !== "boolean" ||
    typeof value.gateRowsRead !== "boolean" ||
    typeof value.boundRepositoriesRead !== "boolean" ||
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
  /**
   * The repository's signed kind:30625 rule records, or null when this view
   * read none (lane L26).
   *
   * Read-optional: null and `[]` both mean "the announcement's own rows are
   * the whole of the rules", which is exactly the answer for a repository
   * whose protection was signed before that kind existed. A record whose
   * author is not a founder is ignored by the native adapter, as it is at the
   * relay's own gate.
   */
  ruleRecords: readonly unknown[] | null;
  /**
   * The mission's active seats, with their roles — arm (C) asks whether a
   * `verifier` seat cleared the report, and an empty list is "not read", not
   * "there are none". The native answer's `seatsRead` says which.
   */
  activeSeats: readonly { actorPubkey: string; role: string }[];
  /**
   * The mission's folded kind 44246 gate rows — arm (B)'s only evidence, and
   * since the 2026-09-03 follow-up ruling half of arm (C)'s too.
   *
   * Already folded, so the provenance check that turns a seat's `observed`
   * claim into `declared` has run. An empty list is "this view read no
   * observations", never "the gates were red" — and on a governed ref it now
   * means no arm but (A) can admit, which is the honest prediction: the relay
   * reads the rows whether or not this view did.
   */
  observedGates: readonly {
    authorPubkey: string;
    source: string;
    gate: string;
    outcome: string;
    headSha: string | null;
    dirty: boolean | null;
  }[];
  /**
   * The gate half of the mission's newest founder-signed policy, or null when
   * this view read none.
   */
  gatePolicy: {
    verifierRequired: boolean | null;
    requiredGates: readonly string[] | null;
  } | null;
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
        ruleRecords: input.ruleRecords === null ? null : [...input.ruleRecords],
        activeSeats: input.activeSeats.map((seat) => ({
          actorPubkey: seat.actorPubkey,
          role: seat.role,
        })),
        observedGates: input.observedGates.map((row) => ({
          authorPubkey: row.authorPubkey,
          source: row.source,
          gate: row.gate,
          outcome: row.outcome,
          headSha: row.headSha,
          dirty: row.dirty,
        })),
        gatePolicy:
          input.gatePolicy === null
            ? null
            : {
                verifierRequired: input.gatePolicy.verifierRequired,
                requiredGates:
                  input.gatePolicy.requiredGates === null
                    ? null
                    : [...input.gatePolicy.requiredGates],
              },
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
  /**
   * Which kind 44245 policy record the admitting arm stood on, in the confirm
   * step — null in every state but `ready` (2026-09-05 refuter, F4).
   *
   * Finding 89 put `policyResolution` and `policyEventId` on the wire and no
   * screen read them, so "the founder set no policy" and "the founder withdrew
   * one" and "this is the founder's own exception" all rendered as the same
   * silence. They are three different reasons a landing is being offered.
   */
  readonly policyLine: string | null;
  /**
   * Whether the mission's repository binding was **read** or assumed, in the
   * confirm step — null in every state but `ready` (finding 91).
   *
   * `boundRepositoriesRead: false` means this answer assumed the mission is
   * bound to the repository on screen. The relay checks the real binding, so a
   * person about to run the command deserves to know which of the two they are
   * looking at.
   */
  readonly bindingLine: string | null;
};

/** `Policy: …` for the confirm step, from the arm's own disclosed evidence. */
function policyLine(evidence: CodingSessionLandEvidence): string {
  if (evidence.policyNotEvaluated !== null) {
    return `Policy: not evaluated — ${evidence.policyNotEvaluated}. A founder's landing is the deliberate exception; nothing here ruled on this commit.`;
  }
  if (evidence.policyResolution === "present") {
    return evidence.policyEventId === null
      ? "Policy: present. The mission's own gate policy applied."
      : `Policy: present (${short(evidence.policyEventId)}). The mission's own gate policy applied.`;
  }
  if (evidence.policyResolution === "withdrawn") {
    return evidence.policyEventId === null
      ? "Policy: withdrawn. The defaults apply because somebody took the policy back."
      : `Policy: withdrawn (${short(evidence.policyEventId)}). The defaults apply because somebody took the policy back.`;
  }
  if (evidence.policyResolution === "absent") {
    return "Policy: absent. The defaults apply because nobody set one.";
  }
  // A resolution this build does not know is disclosed as itself rather than
  // rounded to the friendliest of the three.
  return `Policy: ${evidence.policyResolution || "not evaluated"}.`;
}

/** `Bound repositories: …` for the confirm step (finding 91). */
function bindingLine(read: boolean): string {
  return read
    ? "Bound repositories: read. The rule checked this mission's own binding."
    : "Bound repositories: assumed. This view did not read the mission's binding, so it assumed the repository on screen; the relay checks the real one.";
}

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
 * The disclosure a refusal carries when this view read no gate rows at all.
 *
 * Arm (C) has needed those rows since the 2026-09-03 follow-up ruling, so a
 * refusal can now name a gate that "has no observed green row" for two very
 * different reasons: nobody ran it, or nobody here read the answer. The
 * relay's own copy cannot tell them apart — it always read — so the screen
 * says which one this is rather than letting the stronger reading stand.
 */
export const CODING_SESSION_LAND_NO_GATE_ROWS_READ =
  "This view read no gate rows for this mission, so a sentence about a gate " +
  "with no observed green row is a fact about what reached this screen, not " +
  "about what the mission ran.";

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
  // Any founder may set or remove a rule with a signed rule record (kind
  // 30625, lane L26); the announcement's own rows still belong to its signer.
  // This line used to say the signer alone could rewrite the rules, which was
  // true when it was written and is false now.
  const rules =
    result.rulesSigner === null
      ? ""
      : ` Any founder can set or remove a rule; the announcement's own rules stay with ${resolveWho(result.rulesSigner)}.`;
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
    // Both are confirm-step lines: they describe what an *admission* stood on,
    // and there is no admission in the other three states.
    policyLine: null,
    bindingLine: null,
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
      approvalSentence:
        evidence.arm === "founder"
          ? `You are a founder of this repository, so the require-verdict rule on ${CODING_SESSION_LAND_REF} admits your push with no verdict at all. Nothing here has ruled on this commit.`
          : evidence.arm === "observed-gates"
            ? `Gates observed green on ${evidence.headSha.slice(0, 8)} — ready. ${evidence.observedGates.join(", ")} were each watched passing on this exact commit, over a clean worktree, by this mission's own provider. No person has ruled on it, and this mission requires no verifier.`
            : `Approved by ${input.resolveWho(evidence.dispositionAuthorPubkey)} in disposition ${short(evidence.dispositionEventId)}, over report ${short(evidence.reportEventId)}, and ${input.resolveWho(evidence.verifierPubkey)} did not refute it (refutation ${short(evidence.refutationEventId)}), over ${evidence.observedGates.join(", ")} observed green on this exact commit. The relay's require-verdict rule admits this commit on ${CODING_SESSION_LAND_REF}.`,
      notRunSentence: CODING_SESSION_LAND_NOT_RUN_SENTENCE,
      sentence: null,
      policyLine: policyLine(evidence),
      bindingLine: bindingLine(result.boundRepositoriesRead),
    };
  }

  return {
    ...base,
    state: "refused",
    // §1j's string verbatim behind §1l's prefix. A paraphrase would drift from
    // the words the relay will actually print at push time.
    //
    // L27: the rule can now refuse over gate rows, so a screen that read none
    // must not let "no observed green row" read as a fact about the mission.
    // The disclosure is a **second sentence** rather than an edit to the
    // first, so §1j's copy stays verbatim.
    sentence: `Not ready to land: ${result.refusalReason ?? "the rule refused this commit and gave no reason, which is itself a defect worth reporting."}${result.gateRowsRead ? "" : ` ${CODING_SESSION_LAND_NO_GATE_ROWS_READ}`}`,
  };
}
