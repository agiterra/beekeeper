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
import { publishCodingSessionCommand } from "./codingSessionCommand";
import {
  codingSessionHireRefusalNotice,
  type CodingSessionHireAnswer,
} from "./codingSessionHireAnswer";
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
  if (target) {
    await publishCodingSessionCommand(
      {
        channelId: disclosure.channelId,
        commandId: deps.newTurnCommandId(),
        target,
        text: disclosure.text,
        deliver: "boundary",
      },
      { publisher: deps.publisher, signer: deps.signer },
    ).catch(() => {});
  }
  await publishCodingSessionLaneMessage(
    {
      channelId: disclosure.channelId,
      sessionRef: disclosure.sessionRef,
      content: disclosure.notice,
    },
    { publisher: deps.publisher, signer: deps.signer },
  ).catch(() => {});
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
