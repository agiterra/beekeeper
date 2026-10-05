import * as React from "react";

import {
  type CodingSessionSandboxReport,
  codingSessionSandboxFromTranscript,
} from "@/features/coding-sessions/lib/codingSessionBoundaryStatus";
import type { CodingSessionExecution } from "@/features/coding-sessions/lib/codingSessionTypes";
import type { CodingSessionUmbrellaParticipant } from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";

import { codingSessionSessionFacts } from "./CodingSessionWorkspaceSessionFacts";

/**
 * An umbrella execution's session facts and boundary across every generation
 * (SV-16, SV-17). The open Mission's composer chip, its per-seat tags and the
 * closed Mission's footer all read a seat's boundary through
 * {@link codingSessionExecutionSandbox} or its cached form here, so a seat
 * that ran with full access before a resume says "Was full access" in every
 * one of them, open or closed — one fact, one answer.
 *
 * This module imports no component, so the seat-sandbox picker and the
 * umbrella session facts can both depend on it without a cycle.
 */

/** Each generation's transcript, oldest first: prior generations, then the running one. */
export function codingSessionGenerationTranscripts(
  execution: CodingSessionExecution,
): readonly (readonly TranscriptItem[])[] {
  return [
    ...execution.priorGenerations.map((record) => record.transcript),
    execution.activeGeneration.transcript,
  ];
}

/** One generation's session facts, keyed on the transcript they were read from. */
export type CodingSessionGenerationFacts = {
  transcript: readonly TranscriptItem[];
  facts: readonly TranscriptItem[];
};

/** Every generation's session facts, and all of them combined, oldest first. */
export type CodingSessionUmbrellaGenerationFactsResult = {
  generations: readonly CodingSessionGenerationFacts[];
  facts: readonly TranscriptItem[];
};

/**
 * The session facts of every generation, oldest first, read through the
 * transcript model's session-fact classifier as the single workspace does.
 *
 * `previous` is the last call's result. A generation whose transcript is the
 * same reference is not read again — a streamed item replaces only the
 * running generation's transcript — and the combined `facts` keeps the
 * previous reference when no fact arrived, so the derivations keyed on it do
 * not re-run for the streamed items that are not facts (nearly all of them).
 */
export function codingSessionUmbrellaGenerationFacts(
  transcripts: readonly (readonly TranscriptItem[])[],
  previous: CodingSessionUmbrellaGenerationFactsResult | null,
): CodingSessionUmbrellaGenerationFactsResult {
  const previousGenerations = previous?.generations ?? [];
  let changed =
    previous === null || previousGenerations.length !== transcripts.length;
  const generations = transcripts.map((transcript, index) => {
    const cached = previousGenerations[index];
    if (cached && cached.transcript === transcript) return cached;
    // SV-46: a streamed append re-reads only the appended tail.
    const facts = codingSessionSessionFacts(
      transcript,
      cached?.facts ?? null,
      cached?.transcript ?? null,
    );
    if (facts !== cached?.facts) changed = true;
    return { transcript, facts };
  });
  if (!changed && previous) return { generations, facts: previous.facts };
  const facts = generations.flatMap((generation) => generation.facts);
  if (
    previous &&
    previous.facts.length === facts.length &&
    previous.facts.every((item, index) => item === facts[index])
  ) {
    return { generations, facts: previous.facts };
  }
  return { generations, facts };
}

/**
 * An execution's boundary across every generation, not only the running one:
 * the newest disclosure is what it last reported, and every earlier one —
 * including a prior generation that ran with full access before a resume
 * came back sandboxed — is an earlier period, so "Was full access" is said.
 */
export function codingSessionExecutionSandbox(
  execution: CodingSessionExecution,
): CodingSessionSandboxReport {
  return codingSessionSandboxFromTranscript(
    codingSessionUmbrellaGenerationFacts(
      codingSessionGenerationTranscripts(execution),
      null,
    ).facts,
  );
}

/** One execution's cached boundary: the generation facts it was read from. */
type CodingSessionExecutionSandboxEntry = {
  generations: CodingSessionUmbrellaGenerationFactsResult;
  report: CodingSessionSandboxReport;
};

/**
 * Every execution seat's boundary report by execution key, and the
 * per-generation cache the next call reads from.
 */
export type CodingSessionExecutionSandboxes = {
  cache: ReadonlyMap<string, CodingSessionExecutionSandboxEntry>;
  reports: ReadonlyMap<string, CodingSessionSandboxReport>;
};

/**
 * {@link codingSessionExecutionSandbox} for every execution seat, reusing
 * `previous` (the last call's result): a generation whose transcript is the
 * same reference is not read again, and a seat whose combined facts kept
 * their reference keeps its report — so a streamed item costs one classifier
 * pass over the running generation that changed and nothing else, and
 * `reports` keeps its reference when no seat's report changed.
 */
export function codingSessionExecutionSandboxes(
  participants: readonly CodingSessionUmbrellaParticipant[],
  previous: CodingSessionExecutionSandboxes | null,
): CodingSessionExecutionSandboxes {
  const cache = new Map<string, CodingSessionExecutionSandboxEntry>();
  const reports = new Map<string, CodingSessionSandboxReport>();
  for (const participant of participants) {
    if (participant.kind !== "execution") continue;
    const cached = previous?.cache.get(participant.executionKey) ?? null;
    const generations = codingSessionUmbrellaGenerationFacts(
      codingSessionGenerationTranscripts(participant.execution),
      cached?.generations ?? null,
    );
    const report =
      cached && cached.generations.facts === generations.facts
        ? cached.report
        : codingSessionSandboxFromTranscript(generations.facts);
    cache.set(participant.executionKey, { generations, report });
    reports.set(participant.executionKey, report);
  }
  const held = previous?.reports;
  if (
    held &&
    held.size === reports.size &&
    [...reports].every(([key, report]) => held.get(key) === report)
  ) {
    return { cache, reports: held };
  }
  return { cache, reports };
}

/**
 * {@link codingSessionExecutionSandboxes} for a live umbrella, by execution
 * key. Keyed on each generation's transcript reference rather than on the
 * participants (new objects per streamed item), so the map keeps its
 * reference until some seat's boundary facts change.
 */
export function useCodingSessionExecutionSandboxes(
  participants: readonly CodingSessionUmbrellaParticipant[],
): ReadonlyMap<string, CodingSessionSandboxReport> {
  const previousRef = React.useRef<CodingSessionExecutionSandboxes | null>(
    null,
  );
  // Read during render, written only once the render commits (SV-45): a
  // render React throws away (a suspended or interrupted one, StrictMode's
  // second pass) must not become the next render's cache. The function is
  // pure in `previous`, so reading the last committed result is always safe.
  const next = codingSessionExecutionSandboxes(
    participants,
    previousRef.current,
  );
  React.useLayoutEffect(() => {
    previousRef.current = next;
  });
  return next.reports;
}
