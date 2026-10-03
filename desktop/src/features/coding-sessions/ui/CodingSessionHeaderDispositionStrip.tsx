import * as React from "react";

import {
  formatCodingSessionHireTally,
  summarizeCodingSessionHireOutcomes,
  type CodingSessionHireOutcomeState,
} from "@/features/coding-sessions/lib/codingSessionHireAnswer";
import { useCodingSessionHireOutcomes } from "@/features/coding-sessions/hooks/useCodingSessionHire";
import type { CodingSessionReachabilityResolver } from "@/features/coding-sessions/hooks/useCodingSessionProviderReachability";
import type { CodingSessionUmbrellaRecord } from "@/features/coding-sessions/lib/codingSessionTypes";
import {
  formatCodingSessionDispositionLine,
  listCodingSessionUmbrellaDispositions,
  type CodingSessionActorNameResolver,
} from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import { deriveCodingSessionExecutionStatus } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";

// Split out of `CodingSessionHeader.tsx` (1,000-line ceiling). Unchanged in
// the move; the header re-exports it so existing imports keep working.

/**
 * The umbrella's disposition strip: one line per execution, the lead's first.
 *
 * It sits directly under the header because it answers a header question —
 * *is this team still working?* — that the aggregate status badge cannot: a
 * lead that has delivered its verdict and a builder still standing by are one
 * "Working" between them (ledger 77, "Umbrella UI (a)").
 *
 * Every line comes from the 44223 facts the umbrella already holds; nothing
 * here fetches. The status is the same reachability-demoted one the focus
 * chips use, so a provider nothing is answering for can never read `live`, and
 * an execution with no transcript says so rather than reporting an age it does
 * not have.
 *
 * Since 2026-08-30 it carries one more line: **hires**. This host answers
 * `session.hire` in the app shell, out of sight of every session screen, and
 * until now the outcomes it produced were discarded by the runner that mounted
 * it — a hire could be read, judged and thrown away with nothing on screen at
 * all (ledger draft 97, live). The counts are the only place a person can see
 * that this computer is answering hires, so they sit next to the seats those
 * hires produce. Nothing has happened → no line.
 */
export function CodingSessionDispositionStrip({
  actorNames,
  canSteer = false,
  hireOutcomes,
  nowMs,
  resolveReachability,
  umbrella,
}: {
  /** Resolves a seat's actor pubkey to a display name, when one is known. */
  actorNames?: CodingSessionActorNameResolver;
  /**
   * Whether this viewer may prompt executions. It chooses which of W1's two
   * waiting strings a waiting seat reads, and nothing else.
   */
  canSteer?: boolean;
  /**
   * The hire outcomes to count. Defaults to what this app's hire host has
   * published, which is the only source in the running app; supplied directly
   * by tests, which have no host mounted.
   */
  hireOutcomes?: readonly {
    state: CodingSessionHireOutcomeState;
    detail: string | null;
  }[];
  /** Fixed clock for tests; defaults to now at render time. */
  nowMs?: number;
  resolveReachability: CodingSessionReachabilityResolver;
  umbrella: CodingSessionUmbrellaRecord;
}) {
  const items = React.useMemo(
    () =>
      listCodingSessionUmbrellaDispositions(
        umbrella,
        (execution) =>
          deriveCodingSessionExecutionStatus(
            execution,
            resolveReachability(execution.activeGeneration.commandTarget),
          ),
        actorNames,
        canSteer,
      ),
    [actorNames, canSteer, resolveReachability, umbrella],
  );
  const published = useCodingSessionHireOutcomes();
  const hires = hireOutcomes ?? published;
  const hireTally = React.useMemo(
    () => summarizeCodingSessionHireOutcomes(hires),
    [hires],
  );
  const hireLine = formatCodingSessionHireTally(hireTally);
  if (items.length === 0 && hireLine === null) return null;
  const at = nowMs ?? Date.now();
  return (
    <ul
      aria-label="Session team disposition"
      className="flex flex-wrap items-center gap-x-4 gap-y-0.5 border-b border-border/60 bg-background/70 px-4 py-1 text-2xs text-muted-foreground"
      data-testid="coding-session-disposition-strip"
    >
      {items.map((item) => (
        <li
          className="min-w-0 truncate"
          data-testid="coding-session-disposition-row"
          key={item.executionKey}
        >
          {formatCodingSessionDispositionLine(item, at)}
        </li>
      ))}
      {hireLine === null ? null : (
        <li
          className="min-w-0 truncate"
          data-testid="coding-session-hires-row"
          // The newest reason, on hover. One line on the strip cannot carry a
          // relay sentence, and a count with no way to reach the reason is a
          // number that tells you something is wrong and nothing else.
          title={hireTally.lastReason ?? undefined}
        >
          {hireLine}
        </li>
      )}
    </ul>
  );
}
