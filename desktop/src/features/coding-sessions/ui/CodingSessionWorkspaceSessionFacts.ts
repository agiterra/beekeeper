import * as React from "react";

import { codingSessionSandboxFromTranscript } from "@/features/coding-sessions/lib/codingSessionBoundaryStatus";
import { isCodingSessionSessionFactItem } from "@/features/coding-sessions/lib/codingSessionTranscriptModelPredicates";
import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";

import type { CodingSessionComposerSandbox } from "./CodingSessionComposerSandboxChip";
import {
  type CodingSessionContinuityRow,
  codingSessionContinuityRows,
} from "./CodingSessionHeaderDetailsContinuity";
import type { CodingSessionFullAccess } from "./useCodingSessionFullAccess";

/**
 * The session facts in a transcript — its continuity and boundary rows — in
 * order, by the transcript model's own classifier
 * (`isCodingSessionSessionFactItem`, the one that fills
 * `CodingSessionTranscriptModel.sessionFacts`), so Details, the chip and the
 * transcript agree on which rows these are.
 *
 * Returns `previous` itself when the facts are the same items, so the
 * derivations keyed on it do not re-run for a streamed item that is not a
 * fact — which is nearly every item.
 *
 * `previousTranscript` is the transcript `previous` was read from. When this
 * transcript extends it — every earlier item the same reference, which is
 * what a streamed append leaves — only the appended tail is classified
 * (SV-46); the result is the same as reading the whole transcript again.
 * Anything else (an item replaced, the list shortened) is read in full.
 */
export function codingSessionSessionFacts(
  transcript: readonly TranscriptItem[],
  previous: readonly TranscriptItem[] | null,
  previousTranscript: readonly TranscriptItem[] | null = null,
): readonly TranscriptItem[] {
  if (previous && previousTranscript) {
    const start = codingSessionExtendedLength(previousTranscript, transcript);
    if (start !== null) {
      let facts: TranscriptItem[] | null = null;
      for (let index = start; index < transcript.length; index += 1) {
        const item = transcript[index];
        if (!isCodingSessionSessionFactItem(item)) continue;
        facts ??= [...previous];
        facts.push(item);
      }
      return facts ?? previous;
    }
  }
  const facts: TranscriptItem[] = [];
  for (const item of transcript) {
    if (isCodingSessionSessionFactItem(item)) facts.push(item);
  }
  if (
    previous &&
    previous.length === facts.length &&
    previous.every((item, index) => item === facts[index])
  ) {
    return previous;
  }
  return facts;
}

/**
 * `previous.length` when `next` keeps every item of `previous`, by reference
 * and in place, and adds zero or more after it; otherwise `null`. A reference
 * comparison per earlier item, far cheaper than classifying it again.
 */
export function codingSessionExtendedLength(
  previous: readonly TranscriptItem[],
  next: readonly TranscriptItem[],
): number | null {
  if (next.length < previous.length) return null;
  if (next === previous) return previous.length;
  for (let index = previous.length - 1; index >= 0; index -= 1) {
    if (next[index] !== previous[index]) return null;
  }
  return previous.length;
}

/**
 * The session-level facts the transcript no longer shows inline (decision D3):
 * continuity for Details (SV-16) and the sandbox, with its earlier periods,
 * for the composer chip (SV-17). Both are read from the same transcript items
 * every viewer has, so a teammate on another machine sees what the host's own
 * person sees.
 *
 * `facts` is the transcript model's own `sessionFacts`
 * (`useStableCodingSessionTranscriptModel`), which the workspace derives once
 * and shares with the transcript: no pass over the transcript happens here,
 * and the stabilised list keeps its reference across a streamed item that is
 * not a fact, so the row derivations below re-run only when a fact arrived.
 */
export function useCodingSessionWorkspaceSessionFacts(
  facts: readonly TranscriptItem[],
  fullAccess: CodingSessionFullAccess | null,
): {
  continuity: readonly CodingSessionContinuityRow[];
  sandbox: CodingSessionComposerSandbox;
} {
  const continuity = React.useMemo(
    () => codingSessionContinuityRows(facts),
    [facts],
  );
  const report = React.useMemo(
    () => codingSessionSandboxFromTranscript(facts),
    [facts],
  );
  const granted = fullAccess?.granted ?? null;
  const pending = fullAccess?.pending ?? null;
  const error = fullAccess?.error ?? null;
  const toggle = fullAccess?.toggle;
  const sandbox = React.useMemo<CodingSessionComposerSandbox>(
    () => ({
      report,
      local: granted === null ? null : { granted, pending, error, toggle },
    }),
    [error, granted, pending, report, toggle],
  );
  return { continuity, sandbox };
}
