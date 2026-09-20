/**
 * What a host says about a hire, and to whom.
 *
 * Moved out of `hooks/useCodingSessionHire.ts` unchanged when that file
 * reached the repository's 1000-line ceiling, the same way
 * `codingSessionHireOutcomeStore.ts` was. It is a coherent unit: the two
 * places every answer is published, the refusal that goes through them, and
 * the outcome every non-seated answer is recorded as.
 */
import type {
  CodingSessionHireDeps,
  CodingSessionHireOutcome,
  UseCodingSessionHireInput,
} from "../hooks/useCodingSessionHire";
import {
  isCodingSessionHostAnswerTagUnsupportedRejection,
  publishCodingSessionCommand,
  type CodingSessionCommandTarget,
} from "./codingSessionCommand";
import {
  codingSessionHireRefusalNotice,
  type CodingSessionHireAnswer,
} from "./codingSessionHireAnswer";
import {
  CODING_SESSION_HOST_ANSWER_DOWNGRADE_NOTE,
  markCodingSessionHostNoticeText,
  markCodingSessionHostTurnText,
} from "./codingSessionHireHostNotice";
import { codingSessionHireRequesterLabel } from "./codingSessionHireSeat";
import {
  codingSessionHireRequesterStanding,
  type CodingSessionHireRequest,
} from "./codingSessionHireWire";
import {
  formatCodingSessionHireRefusal,
  type CodingSessionHireRefusalCode,
} from "./codingSessionHirePolicy";
import { publishCodingSessionLaneMessage } from "./codingSessionLanePublish";

/**
 * Say one fact twice: privately to the seat that asked, and in the umbrella.
 *
 * To the requester so it can act, and to the umbrella so the person sees it.
 * Neither is allowed to fail the other: a lead that heard nothing would wait
 * out its whole turn budget on an answer that was published and dropped.
 *
 * The turn to the requester is **always** published, exactly as before
 * ledger 178: `bee sessions hire` reads this same event directly off the
 * relay to answer its own poll
 * (`find_hire_refusal`/`wait_for_hire`/`read_hire_answer`,
 * `crates/buzz-cli/src/commands/sessions/crew.rs`,
 * `crates/buzz-cli/src/commands/sessions/crew_cmds.rs`) — it is the CLI's
 * *answer channel*, not a redundant echo, and suppressing it (an earlier,
 * wrong version of this fix) would have made every fast refusal report
 * `unconfirmed` instead of `refused`. What must not happen is the *live
 * seat*, still running, reading the same words a second time whenever the
 * provider's mailbox gets to it, as though a person just typed them
 * (ledger 178(b)). Two independent things fix that instead of suppressing
 * the publish: the text carries {@link markCodingSessionHostTurnText}'s
 * marker, and the command itself carries `hostAnswer: true`
 * (`CODING_SESSION_HOST_ANSWER_TAG_NAME` in `codingSessionCommand.ts`), which
 * the provider's turn intake reads to record the answer in the transcript
 * without ever opening a turn from it
 * (`crates/buzz-session-provider/src/lib.rs`).
 *
 * **A desktop must work against a relay one release behind (ledger 192).**
 * Lane 181 taught the relay's ingest allowlist to accept the
 * `buzz-host-answer` tag alongside the desktop that writes it, but the
 * production relay at hive.agiterra.org still ran the 2026-09-18 image as of
 * 2026-09-20, whose allowlist refuses any tag it does not recognize —
 * `invalid: unsupported coding-session command tag`. Against that relay
 * *every* tagged turn published here was rejected at ingest, so the
 * requesting seat never saw an answer at all and `bee sessions hire` waited
 * out its full window and reported `unconfirmed` — exactly ledger 169's
 * failure, reopened by 181's own fix. {@link publishCodingSessionHostAnswerTurn}
 * retries once, untagged, on that one rejection and discloses the downgrade;
 * it is owed only until hive reports a `software_commit` that contains 181,
 * and should be removed then.
 */
export async function discloseCodingSessionHire(
  disclosure: {
    channelId: string;
    sessionRef: string;
    requesterPubkey: string;
    /** The 44220 text the requesting seat receives. */
    text: string;
    /** The umbrella's own line for the same fact. */
    notice: string;
  },
  input: UseCodingSessionHireInput,
  deps: CodingSessionHireDeps,
): Promise<void> {
  const target = input.targetForActor(
    disclosure.channelId,
    disclosure.requesterPubkey,
  );
  const downgraded = target
    ? await publishCodingSessionHostAnswerTurn(disclosure, target, deps)
    : false;
  const notice = downgraded
    ? `${disclosure.notice} ${CODING_SESSION_HOST_ANSWER_DOWNGRADE_NOTE}`
    : disclosure.notice;
  await publishCodingSessionLaneMessage(
    {
      channelId: disclosure.channelId,
      sessionRef: disclosure.sessionRef,
      content: markCodingSessionHostNoticeText(notice),
    },
    { publisher: deps.publisher, signer: deps.signer },
  ).catch(() => {});
}

/**
 * Publish the requester-addressed turn, falling back to an untagged republish
 * exactly once when — and only when — an older relay refuses the tag itself.
 *
 * Returns whether the fallback was the one that landed, so the caller can say
 * so in the umbrella notice. Never throws: a turn this host cannot get
 * published, tagged or not, is logged to the host's own console rather than
 * blocking the umbrella notice that follows it — the same "never drop the
 * refusal" rule that already governs every other failure on this path, and
 * unchanged from before this fix for any rejection other than the one named
 * above.
 */
async function publishCodingSessionHostAnswerTurn(
  disclosure: { channelId: string; text: string },
  target: CodingSessionCommandTarget,
  deps: CodingSessionHireDeps,
): Promise<boolean> {
  const text = markCodingSessionHostTurnText(disclosure.text);
  let taggedError: unknown;
  try {
    await publishCodingSessionCommand(
      {
        channelId: disclosure.channelId,
        commandId: deps.newTurnCommandId(),
        target,
        text,
        deliver: "boundary",
        hostAnswer: true,
      },
      { publisher: deps.publisher, signer: deps.signer },
    );
    return false;
  } catch (error) {
    taggedError = error;
  }
  if (!isCodingSessionHostAnswerTagUnsupportedRejection(taggedError)) {
    console.error("coding-session host answer turn was refused", taggedError);
    return false;
  }
  try {
    await publishCodingSessionCommand(
      {
        channelId: disclosure.channelId,
        // A fresh id and a fresh signature for the retry — never the
        // rejected event resent. `hostAnswer` is omitted, not `false`: this
        // relay's allowlist does not know the tag exists, so the event must
        // carry none of it, exactly the shape an ordinary turn already has.
        commandId: deps.newTurnCommandId(),
        target,
        text,
        deliver: "boundary",
      },
      { publisher: deps.publisher, signer: deps.signer },
    );
    return true;
  } catch (untaggedError) {
    console.error(
      "coding-session host answer turn was refused both tagged and untagged",
      taggedError,
      untaggedError,
    );
    return false;
  }
}

export async function publishRefusal(
  request: CodingSessionHireRequest,
  answer: Extract<CodingSessionHireAnswer, { kind: "refused" }>,
  input: UseCodingSessionHireInput,
  deps: CodingSessionHireDeps,
): Promise<void> {
  await discloseCodingSessionHire(
    {
      channelId: request.channelId,
      sessionRef: request.action.sessionRef,
      requesterPubkey: request.requesterPubkey,
      text: answer.text,
      notice: codingSessionHireRefusalNotice({
        role: request.action.role,
        // Named, not "A seat". The hire carries `requestedBy`, this host
        // compares it with the signer itself — the relay does not
        // (POLICY.md 5) — and the three answers are three different
        // sentences, including the one that says the claim is disputed.
        requesterLabel: codingSessionHireRequesterLabel({
          standing: codingSessionHireRequesterStanding(request),
          nameFor: (pubkey) =>
            input.agents.find((agent) => agent.pubkey === pubkey)?.name ?? null,
        }),
        text: answer.text,
      }),
    },
    input,
    deps,
  );
}

/**
 * Refuse a hire with a code and a reason of this host's own making.
 *
 * {@link publishRefusal} answers a *decision* — a refusal the planner
 * produced, with its text already written. This answers a hire that got past
 * the decision and then failed on this computer: the host writes the same
 * `hire refused: <CODE> — <reason>` sentence `bee sessions hire` parses
 * structurally, says it to the requesting seat and in the umbrella, and the
 * caller records the outcome.
 *
 * It exists because such a failure had no answer at all. On 2026-09-19 the
 * seat's agents clone was refused by git, staging threw, and the hire was
 * recorded host-locally as an `error` and published nowhere: the lead waited
 * out its whole window (ledger 169). A hire that gets no answer is a crash
 * with better manners.
 */
export async function refuseCodingSessionHireWithCode(
  request: CodingSessionHireRequest,
  refusal: {
    code: CodingSessionHireRefusalCode;
    /** The host's own words, verbatim — never a summary of them. */
    reason: string;
  },
  input: UseCodingSessionHireInput,
  deps: CodingSessionHireDeps,
): Promise<string> {
  const text = formatCodingSessionHireRefusal(refusal);
  await discloseCodingSessionHire(
    {
      channelId: request.channelId,
      sessionRef: request.action.sessionRef,
      requesterPubkey: request.requesterPubkey,
      text,
      notice: codingSessionHireRefusalNotice({
        role: request.action.role,
        requesterLabel: codingSessionHireRequesterLabel({
          standing: codingSessionHireRequesterStanding(request),
          nameFor: (pubkey) =>
            input.agents.find((agent) => agent.pubkey === pubkey)?.name ?? null,
        }),
        text,
      }),
    },
    input,
    deps,
  );
  return text;
}

/**
 * Answer a hire whose seat this host cut a tree for and then could not stage.
 *
 * Two things in one, because neither is right without the other: the worktree
 * cut for a seat that will never exist is removed — by the host's own prune,
 * which takes the § 4.11 agents clone recorded against it — and the hire is
 * refused `HIRE_SEAT_STAGING_FAILED` with the host's error text verbatim and
 * what happened to the tree. The tree goes first so the refusal can say so.
 *
 * Nothing here is a judgement about *what* failed: the host's own words are
 * the only thing that names the part (git refusing the clone, an unreachable
 * keyring, a role pack this computer cannot read), and summarizing them would
 * throw away the one fact the operator needs (ledger 169).
 */
export async function refuseCodingSessionHireForSeatingFailure(
  request: CodingSessionHireRequest,
  seating: {
    /** The host's own error text. */
    failure: string;
    sessionRef: string;
    seatLabel: string;
    /** Only for the sentence when the prune itself fails. */
    worktreePath: string;
  },
  input: UseCodingSessionHireInput,
  deps: CodingSessionHireDeps,
): Promise<void> {
  let disposal: string;
  try {
    disposal = await deps.disposeSeatWorktree({
      sessionRef: seating.sessionRef,
      seatLabel: seating.seatLabel,
    });
  } catch (error: unknown) {
    disposal = `the worktree at ${seating.worktreePath} could not be removed: ${
      error instanceof Error ? error.message : String(error)
    }`;
  }
  await refuseCodingSessionHireWithCode(
    request,
    {
      code: "HIRE_SEAT_STAGING_FAILED",
      reason: `${seating.failure} — no seat was created; ${disposal}`,
    },
    input,
    deps,
  );
}

export function outcomeOf(
  request: CodingSessionHireRequest,
  state: CodingSessionHireOutcome["state"],
  detail: string | null,
): CodingSessionHireOutcome {
  return {
    commandId: request.commandId,
    channelId: request.channelId,
    sessionRef: request.action.sessionRef,
    role: request.action.role,
    state,
    detail,
    seatCommandId: null,
    granted: false,
    seatActor: null,
    requesterLabel: null,
    hostPubkey: null,
  };
}
