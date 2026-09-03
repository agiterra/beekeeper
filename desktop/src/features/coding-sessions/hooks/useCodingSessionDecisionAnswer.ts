import * as React from "react";

import {
  publishCodingSessionDecisionAnswer,
  codingSessionTeamTransactionCapabilities,
  type CodingSessionDecisionAnswerDeps,
  type CodingSessionDecisionAnswerDraft,
  type CodingSessionTeamTransactionCapabilities,
} from "@/features/coding-sessions/lib/codingSessionTeamTransactionPublish";

/** Longest free-text choice a founder may type, before anything is signed. */
export const DEFAULT_CODING_SESSION_CHOICE_MAX_BYTES = 2 * 1024;
/** Longest note, before anything is signed. */
export const DEFAULT_CODING_SESSION_NOTE_MAX_BYTES = 8 * 1024;
/** Longest §1k condition, before anything is signed. */
export const DEFAULT_CODING_SESSION_CONDITION_MAX_BYTES = 512;

function byteLength(value: string): number {
  return new TextEncoder().encode(value).length;
}

/**
 * The refusal a founder sees when a field is too long, naming the field.
 *
 * These run **before** the native builder, so a person is told which field to
 * shorten rather than watching a publish fail; `buzz-core` still refuses
 * anything that gets past them, in its own words, before a signature exists.
 */
export function codingSessionAnswerFieldRefusal(
  draft: CodingSessionDecisionAnswerDraft,
  bounds: {
    choiceMaxBytes: number;
    noteMaxBytes: number;
    conditionMaxBytes: number;
    /**
     * How many options the request declared, when the caller knows.
     *
     * REVIEW-L8 F12: an index is the one field nothing bounded — not for
     * range, not for integrality. Not constructible from the buttons, which
     * map over the signed options; bounded here so the field has an owner.
     */
    declaredOptions?: number;
  },
): string | null {
  if (typeof draft.choice === "number") {
    if (!Number.isSafeInteger(draft.choice) || draft.choice < 0) {
      return "The answer's choice is not an option index, so nothing was signed.";
    }
    if (
      bounds.declaredOptions !== undefined &&
      draft.choice >= bounds.declaredOptions
    ) {
      return `The answer's choice names option ${draft.choice + 1}, and this request declared ${bounds.declaredOptions}, so nothing was signed.`;
    }
  }
  if (
    typeof draft.choice === "string" &&
    byteLength(draft.choice) > bounds.choiceMaxBytes
  ) {
    return `The answer's choice is longer than ${bounds.choiceMaxBytes} bytes, so nothing was signed. Shorten it and answer again.`;
  }
  if (draft.note !== null && byteLength(draft.note) > bounds.noteMaxBytes) {
    return `The answer's note is longer than ${bounds.noteMaxBytes} bytes, so nothing was signed. Shorten it and answer again.`;
  }
  if (
    draft.condition !== null &&
    byteLength(draft.condition) > bounds.conditionMaxBytes
  ) {
    return `The answer's condition is longer than ${bounds.conditionMaxBytes} bytes, so nothing was signed. Shorten it and answer again.`;
  }
  return null;
}

/** What the queue holds while a founder is answering one ruling. */
export type CodingSessionDecisionAnswerState = {
  /** The request id currently being published, or null. */
  readonly pendingRequestId: string | null;
  /** The relay's own words after a rejected publish, keyed by request id. */
  readonly errors: Readonly<Record<string, string>>;
  /**
   * The answer event id a successful publish left on the wire, keyed by
   * request id.
   *
   * Not "answered": the row only reads `Answered by …` when a **fold** carries
   * the answer. This is the weaker, true claim — an event went out — and it
   * exists because rendering *nothing* on success (fix round 1) let a founder
   * who saw no change sign a second `decision.answer` for the same request
   * (REVIEW-L8 F8).
   */
  readonly published: Readonly<Record<string, string>>;
  /** Which optional keys and bounds this build's core reported, or null. */
  readonly capabilities: CodingSessionTeamTransactionCapabilities | null;
  /** Publish one answer. Resolves false when the relay refused it. */
  answer: (input: {
    channelRef: string;
    sessionRef: string;
    genesisRef: string;
    draft: CodingSessionDecisionAnswerDraft;
    /** How many options the request declared, for the index bound (F12). */
    declaredOptions?: number;
  }) => Promise<boolean>;
};

/**
 * The founder's answer, from the screen that showed the question.
 *
 * The hook owns exactly two things: the in-flight request id, and the relay's
 * own sentence when a publish is refused. It **never** marks a row answered —
 * that comes from the next fold, and a queue that flipped on a resolved
 * promise would claim a ruling the wire does not carry (§1l).
 *
 * The capability probe runs once, on mount, and is not a poll: it asks this
 * build's `buzz-core` which optional keys it accepts so the form can offer the
 * condition field only where the bytes can carry it.
 */
export function useCodingSessionDecisionAnswer(
  deps?: Partial<CodingSessionDecisionAnswerDeps> & {
    publish?: typeof publishCodingSessionDecisionAnswer;
  },
): CodingSessionDecisionAnswerState {
  const [pendingRequestId, setPendingRequestId] = React.useState<string | null>(
    null,
  );
  const [errors, setErrors] = React.useState<Record<string, string>>({});
  const [published, setPublished] = React.useState<Record<string, string>>({});
  const [capabilities, setCapabilities] =
    React.useState<CodingSessionTeamTransactionCapabilities | null>(null);
  const probe = deps?.capabilities ?? codingSessionTeamTransactionCapabilities;
  const publish = deps?.publish ?? publishCodingSessionDecisionAnswer;

  React.useEffect(() => {
    let cancelled = false;
    void probe()
      .then((value) => {
        if (!cancelled) setCapabilities(value);
      })
      // A probe that fails leaves capabilities `null`, which offers no
      // optional field at all — never a guess in either direction.
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [probe]);

  const answer = React.useCallback(
    async (input: {
      channelRef: string;
      sessionRef: string;
      genesisRef: string;
      draft: CodingSessionDecisionAnswerDraft;
      declaredOptions?: number;
    }): Promise<boolean> => {
      const bounds = {
        declaredOptions: input.declaredOptions,
        choiceMaxBytes:
          capabilities?.choiceMaxBytes ??
          DEFAULT_CODING_SESSION_CHOICE_MAX_BYTES,
        noteMaxBytes:
          capabilities?.noteMaxBytes ?? DEFAULT_CODING_SESSION_NOTE_MAX_BYTES,
        conditionMaxBytes:
          capabilities?.conditionMaxBytes ??
          DEFAULT_CODING_SESSION_CONDITION_MAX_BYTES,
      };
      const refusal = codingSessionAnswerFieldRefusal(input.draft, bounds);
      if (refusal !== null) {
        setErrors((current) => ({
          ...current,
          [input.draft.requestRef]: refusal,
        }));
        return false;
      }
      setPendingRequestId(input.draft.requestRef);
      try {
        const accepted = await publish({
          channelRef: input.channelRef,
          sessionRef: input.sessionRef,
          genesisRef: input.genesisRef,
          draft: input.draft,
          deps:
            deps === undefined
              ? undefined
              : (deps as CodingSessionDecisionAnswerDeps),
        });
        setErrors((current) => {
          const next = { ...current };
          delete next[input.draft.requestRef];
          return next;
        });
        setPublished((current) => ({
          ...current,
          [input.draft.requestRef]: accepted.eventId,
        }));
        return true;
      } catch (error) {
        // The relay's own words, prefixed by §1l's sentence. The row stays
        // `Open · held on …` because nothing on the wire changed.
        setErrors((current) => ({
          ...current,
          [input.draft.requestRef]: `The relay did not accept this answer: ${
            error instanceof Error ? error.message : String(error)
          }`,
        }));
        return false;
      } finally {
        setPendingRequestId(null);
      }
    },
    [capabilities, deps, publish],
  );

  return { pendingRequestId, errors, published, capabilities, answer };
}
