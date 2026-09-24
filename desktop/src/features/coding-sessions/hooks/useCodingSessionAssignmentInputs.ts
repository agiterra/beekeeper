import * as React from "react";

import {
  observeCodingSessionAssignmentInputs,
  requeueCodingSessionAssignmentInput,
  type CodingSessionAssignmentInputStatus,
  type ObservedCodingSessionAssignment,
} from "@/features/coding-sessions/lib/codingSessionAssignmentInput";
import {
  codingSessionAssignmentInputStateFromRecord,
  type CodingSessionAssignmentInputState,
} from "@/features/coding-sessions/lib/codingSessionAssignmentInputCopy";

/**
 * Show what this computer did about each verifier's and runner's exact input.
 *
 * # What changed, and why it matters
 *
 * This hook used to *be* the sequence: it read the host's record, decided
 * candidacy, called the checkout command, held the "already attempted" set in
 * a ref, and did all of it inside a `useEffect` that only ran while the
 * Mission surface was mounted. So whether a panel was open decided whether a
 * seat's tree ever reached the commit it was hired to verify — and a relaunch
 * forgot everything. In the 2026-09-20 kettle run the verifier's tree stayed
 * on the README commit and the seat verified a scratch archive by hand
 * (ledger 178(d), 185).
 *
 * The sequence now lives in the host, durably
 * (`desktop/src-tauri/src/coding_sessions/assignment_establishment.rs`): it
 * records an intent, performs the checkout, records the result, finishes at
 * the next launch anything a quit interrupted, and never retries a settled
 * refusal. This hook does two things and nothing else:
 *
 * 1. **It reports what the surface folded.** One call per distinct set of
 *    assignments, handing over the signed fields plus the seat label this
 *    computer cut the tree under — the one fact the host cannot resolve, since
 *    the actor-to-seat mapping lives in the frontend's roster.
 * 2. **It displays the host's answer.** Every state here is read from the
 *    host's durable record; none is computed from a call this window made.
 *
 * Reporting is deliberately **not** gated on the mission panel being the
 * visible surface. Handing over an observation is how the host learns the work
 * exists at all, and making that conditional on a density setting is the class
 * of bug this change exists to remove.
 *
 * Two honest limits:
 *
 * - The host has no relay subscription of its own for kind 44244, so the first
 *   sighting of an assignment still comes from a surface that folded it under
 *   a session's authority context. What no longer depends on a surface is
 *   everything after that: the queue survives the panel and the process.
 * - Nothing here is ordered before the seat's wake — only the lead's CLI wakes
 *   an assignee. The provider's fence is what makes the order irrelevant, and
 *   the copy says so on every established row.
 */

/** One assignment, as the reporter needs it. */
export type CodingSessionAssignmentInputTarget = {
  /** The signed assignment event id. */
  assignmentId: string;
  /** The assignee's actor pubkey, from the signed body. */
  assigneeActor: string | null;
  /** The signed `assigneeRole`. */
  assigneeRole: string | null;
  /** The signed `baseSha`, or null when the assignment named none. */
  baseSha: string | null;
  /** The signed branch, when the assignment carried one. */
  branch?: string | null;
};

/** The outside world, injected so tests need no Tauri host. */
export type CodingSessionAssignmentInputDeps = {
  observe: (
    assignments: readonly ObservedCodingSessionAssignment[],
  ) => Promise<
    | { kind: "statuses"; statuses: CodingSessionAssignmentInputStatus[] }
    | { kind: "unavailable"; message: string }
  >;
  requeue: (input: {
    assignmentId: string;
  }) => Promise<
    | { kind: "status"; status: CodingSessionAssignmentInputStatus }
    | { kind: "none" }
    | { kind: "unavailable"; message: string }
  >;
};

const HOST_DEPS: CodingSessionAssignmentInputDeps = {
  observe: (assignments) => observeCodingSessionAssignmentInputs(assignments),
  requeue: (input) => requeueCodingSessionAssignmentInput(input),
};

export type CodingSessionAssignmentInputs = {
  /** assignment event id → what this host has to say about its input. */
  states: ReadonlyMap<string, CodingSessionAssignmentInputState>;
  /** Ask this computer to establish one assignment's input again. */
  retry: (assignmentId: string) => void;
};

/**
 * The observations to hand the host, in assignment order.
 *
 * Every assignment is reported, builders included: which roles start from a
 * named revision is the host's rule (`role_requires_verification_input` in
 * `buzz-core`), and duplicating it here is how the two would drift. A repeated
 * assignment id is reported once.
 */
export function codingSessionAssignmentInputObservations(
  sessionRef: string,
  assignments: readonly CodingSessionAssignmentInputTarget[],
  resolveSeatLabel: (actor: string) => string | null,
): ObservedCodingSessionAssignment[] {
  const seen = new Set<string>();
  const observations: ObservedCodingSessionAssignment[] = [];
  for (const assignment of assignments) {
    if (seen.has(assignment.assignmentId)) continue;
    seen.add(assignment.assignmentId);
    observations.push({
      assignmentId: assignment.assignmentId,
      sessionRef,
      seatLabel:
        assignment.assigneeActor === null
          ? null
          : resolveSeatLabel(assignment.assigneeActor),
      assigneeRole: assignment.assigneeRole,
      baseSha: assignment.baseSha,
      branch: assignment.branch ?? null,
    });
  }
  return observations;
}

/** The identity of one report, so the same one is not made twice. */
function observationKey(
  observations: readonly ObservedCodingSessionAssignment[],
): string {
  return observations
    .map(
      (observation) =>
        `${observation.assignmentId}|${observation.assigneeRole ?? ""}|${observation.baseSha ?? ""}|${observation.seatLabel ?? ""}`,
    )
    .join("\n");
}

/** The sentence an unreadable host answer leaves behind. */
const HOST_UNREADABLE =
  "This computer could not say what it did about this verification input.";

/**
 * Read one host status as the row's state.
 *
 * Exported so the whole mapping — including which dispositions have no row at
 * all — is testable without React.
 */
export function codingSessionAssignmentInputStateFromStatus(
  status: CodingSessionAssignmentInputStatus,
  options?: {
    /**
     * True when this assignment's actor could not be resolved to a seat
     * label *and* this session holds a worktree cut before this host began
     * naming which actor a seat belonged to — so an `off_host` answer here
     * cannot be told apart from a seat this host really did cut a tree for.
     * See `useCodingSessionSeatWorktreeActors`.
     */
    unattributed?: boolean;
  },
): CodingSessionAssignmentInputState | null {
  if (status.disposition === "not_required") return null;
  if (status.disposition === "unnamed") return { kind: "unnamed" };
  const commit = status.record?.commit ?? "";
  if (status.disposition === "off_host") {
    if (options?.unattributed) {
      return { kind: "unknown-attribution", commit };
    }
    return {
      kind: "refused",
      commit,
      code: "unrecorded_tree",
      message: "No worktree for this seat is recorded on this computer.",
      detail: null,
      changes: null,
    };
  }
  if (status.disposition === "invalid") {
    return {
      kind: "unavailable",
      commit,
      message:
        "This computer could not read this assignment as one it can establish an input for.",
      detail: null,
    };
  }
  if (status.disposition === "recorded" && status.record !== null) {
    return codingSessionAssignmentInputStateFromRecord(status.record);
  }
  // `unreadable`, or a `recorded` disposition with no record behind it: named
  // as unanswerable rather than shown as nothing to do.
  return {
    kind: "unavailable",
    commit,
    message: HOST_UNREADABLE,
    detail: null,
  };
}

export function useCodingSessionAssignmentInputs(input: {
  /** The umbrella's session ref; nothing is reported without one. */
  sessionRef: string | null;
  assignments: readonly CodingSessionAssignmentInputTarget[];
  /** Actor pubkey → the seat label this host cut the tree under. */
  resolveSeatLabel: (actor: string) => string | null;
  /**
   * True when an actor that {@link resolveSeatLabel} could not name might
   * still have a seat this host cut a tree for — see
   * `useCodingSessionSeatWorktreeActors`. Absent (or always false) keeps the
   * old reading: an unresolved actor is off-host.
   */
  isUnattributedActor?: (actor: string) => boolean;
  /** Injected only by tests. */
  deps?: CodingSessionAssignmentInputDeps;
}): CodingSessionAssignmentInputs {
  const { resolveSeatLabel, sessionRef, isUnattributedActor } = input;
  const deps = input.deps ?? HOST_DEPS;
  const [states, setStates] = React.useState<
    ReadonlyMap<string, CodingSessionAssignmentInputState>
  >(() => new Map());

  const reportedRef = React.useRef<string | null>(null);
  const mountedRef = React.useRef(true);
  const depsRef = React.useRef(deps);
  depsRef.current = deps;
  const resolveRef = React.useRef(resolveSeatLabel);
  resolveRef.current = resolveSeatLabel;
  const unattributedRef = React.useRef(isUnattributedActor);
  unattributedRef.current = isUnattributedActor;
  // assignment id → whether that assignment's actor is unresolved *and*
  // ambiguously so. Filled alongside `observations`, so `applyStatuses` can
  // read it without adding a render dependency of its own.
  const unattributedByAssignmentRef = React.useRef<
    ReadonlyMap<string, boolean>
  >(new Map());

  const observations = React.useMemo(() => {
    if (sessionRef === null) {
      unattributedByAssignmentRef.current = new Map();
      return [];
    }
    const built = codingSessionAssignmentInputObservations(
      sessionRef,
      input.assignments,
      (actor) => resolveRef.current(actor),
    );
    const unattributed = new Map<string, boolean>();
    for (const assignment of input.assignments) {
      if (assignment.assigneeActor === null) continue;
      unattributed.set(
        assignment.assignmentId,
        unattributedRef.current?.(assignment.assigneeActor) ?? false,
      );
    }
    unattributedByAssignmentRef.current = unattributed;
    return built;
  }, [input.assignments, sessionRef]);

  React.useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  // A different session is a different question. Adjusted during render so the
  // first render of the new mission never shows the previous one's answers.
  const [heldSession, setHeldSession] = React.useState(sessionRef);
  if (heldSession !== sessionRef) {
    setHeldSession(sessionRef);
    reportedRef.current = null;
    setStates(new Map());
  }

  const applyStatuses = React.useCallback(
    (statuses: readonly CodingSessionAssignmentInputStatus[]) => {
      if (!mountedRef.current) return;
      setStates((previous) => {
        const next = new Map(previous);
        for (const status of statuses) {
          const state = codingSessionAssignmentInputStateFromStatus(status, {
            unattributed:
              unattributedByAssignmentRef.current.get(status.assignmentId) ??
              false,
          });
          if (state === null) next.delete(status.assignmentId);
          else next.set(status.assignmentId, state);
        }
        return next;
      });
    },
    [],
  );

  const applyUnavailable = React.useCallback(
    (assignmentIds: readonly string[], message: string) => {
      if (!mountedRef.current) return;
      setStates((previous) => {
        const next = new Map(previous);
        for (const assignmentId of assignmentIds) {
          next.set(assignmentId, {
            kind: "unavailable",
            commit: "",
            message,
            detail: null,
          });
        }
        return next;
      });
    },
    [],
  );

  React.useEffect(() => {
    if (observations.length === 0) return;
    const key = observationKey(observations);
    if (reportedRef.current === key) return;
    reportedRef.current = key;
    void depsRef.current.observe(observations).then((answer) => {
      if (answer.kind === "statuses") applyStatuses(answer.statuses);
      else {
        // A host that cannot answer is stated, and the report is left
        // un-made so a later render tries once more rather than never.
        reportedRef.current = null;
        applyUnavailable(
          observations.map((observation) => observation.assignmentId),
          answer.message,
        );
      }
    });
  }, [applyStatuses, applyUnavailable, observations]);

  const retry = React.useCallback(
    (assignmentId: string) => {
      void depsRef.current.requeue({ assignmentId }).then((answer) => {
        if (answer.kind === "status") applyStatuses([answer.status]);
        else if (answer.kind === "unavailable") {
          applyUnavailable([assignmentId], answer.message);
        } else {
          applyUnavailable(
            [assignmentId],
            "This computer holds no record of that verification input, so there was nothing to try again.",
          );
        }
      });
    },
    [applyStatuses, applyUnavailable],
  );

  return { states, retry };
}
