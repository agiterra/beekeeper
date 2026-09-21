import type {
  CodingSessionMissionPendingCompletionInput,
  CodingSessionMissionSettlementInput,
} from "../lib/codingSessionMissionInspectorModel";

/** The one link an assignment is waiting for, as a sentence. */
export function awaitingSentence(
  settlement: CodingSessionMissionSettlementInput,
  resolveActorName?: (pubkey: string) => string,
): string {
  // Lane 210: a settled assignment says which rule settled it, in words. An
  // approval that asks the assignee for nothing settles with no receipt, and
  // a bare "settled" beside a null acknowledgement id would leave a reader to
  // infer which of the two happened — the inference this panel exists to end.
  if (settlement.settled) {
    switch (settlement.settledBy) {
      case "approving_disposition_without_ask":
        return "settled by an approving disposition that asked for nothing — no acknowledgement was owed";
      case "acknowledgement":
        return "settled by the assignee's acknowledgement";
      default:
        return "settled";
    }
  }
  const awaiting = settlement.awaiting;
  if (!awaiting) {
    // The fold's own invariant is `awaiting` set exactly while `settled` is
    // false. If that is ever violated the surface says so rather than
    // printing nothing, which is the bug this panel exists to end.
    return "not settled, and this fold did not say which link is missing";
  }
  const who = awaiting.owedByActor
    ? ` ${resolveActorName?.(awaiting.owedByActor) ?? `${awaiting.owedByActor.slice(0, 8)}…`}`
    : "";
  const several =
    awaiting.owedByActor === null && awaiting.link === "disposition"
      ? " (the founder, any active lead seat or any steer-grant holder may give it)"
      : "";
  return `awaiting ${awaiting.link} by ${awaiting.owedByRole}${who}${several}`;
}

/**
 * Where each assignment's approval chain is waiting, and whether a signed
 * completion is being held for a late prerequisite.
 *
 * Ledger 183(g). The Mission panel showed the four older settlement fields
 * and nothing else, so a person reading the app saw the three nulls the CLI
 * had stopped printing — the exact nulls that made the kettle lead conclude a
 * refutation was missing and recall a verifier that owed nothing (178(e)).
 *
 * This renders the native fold's `awaiting` and `pendingCompletion` verbatim
 * and derives neither. A pending completion is shown as *held*, never as a
 * failure and never as a terminal: the same signed record settles on the next
 * read once its last prerequisite lands, with nothing republished and no turn
 * opened.
 */
export function CodingSessionMissionSettlement({
  settlements,
  pendingCompletion,
  resolveActorName,
}: {
  /** `undefined` means no fold ran — unknown, never "everything is settled". */
  settlements?: readonly CodingSessionMissionSettlementInput[];
  pendingCompletion?: CodingSessionMissionPendingCompletionInput | null;
  resolveActorName?: (pubkey: string) => string;
}) {
  if (settlements === undefined) {
    return (
      <p
        className="text-xs text-muted-foreground"
        data-testid="mission-settlement-unknown"
      >
        Settlement unknown: no fold has answered for this mission yet.
      </p>
    );
  }
  const held = pendingCompletion ?? null;
  return (
    <div className="flex flex-col gap-2" data-testid="mission-settlement">
      {held ? (
        <p
          className="text-sm text-amber-600 dark:text-amber-400"
          data-testid="mission-pending-completion"
        >
          A completion is published and held:{" "}
          <span className="text-foreground">{held.reason}</span> It becomes this
          mission&apos;s terminal on the next read once the last prerequisite
          arrives — nothing is republished and no turn is opened.
        </p>
      ) : null}
      {settlements.length === 0 ? (
        <p className="text-xs text-muted-foreground">
          This mission has no active assignment.
        </p>
      ) : (
        <ul className="flex flex-col gap-1">
          {settlements.map((settlement) => (
            <li
              className="flex flex-wrap items-baseline gap-2 text-xs"
              data-assignment-id={settlement.assignmentEventId}
              data-testid="mission-settlement-row"
              key={settlement.assignmentEventId}
            >
              <span className="font-mono text-2xs text-muted-foreground">
                {settlement.assignmentEventId.slice(0, 8)}…
              </span>
              <span
                className={
                  settlement.settled
                    ? "text-emerald-600 dark:text-emerald-400"
                    : "text-foreground"
                }
              >
                {awaitingSentence(settlement, resolveActorName)}
              </span>
              {held?.unsettledAssignmentEventIds.includes(
                settlement.assignmentEventId,
              ) ? (
                <span className="text-2xs text-muted-foreground">
                  named by the held completion
                </span>
              ) : null}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
