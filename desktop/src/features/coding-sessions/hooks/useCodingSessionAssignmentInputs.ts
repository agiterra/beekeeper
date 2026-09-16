import * as React from "react";

import {
  establishCodingSessionAssignmentInput,
  isCodingSessionInputBoundRole,
  readCodingSessionAssignmentInputRecord,
  type CodingSessionAssignmentInputOutcome,
  type CodingSessionAssignmentInputRecordRead,
} from "@/features/coding-sessions/lib/codingSessionAssignmentInput";
import {
  codingSessionAssignmentInputState,
  codingSessionAssignmentInputStateFromRecord,
  type CodingSessionAssignmentInputState,
} from "@/features/coding-sessions/lib/codingSessionAssignmentInputCopy";
import {
  listCodingSessionSeatWorktrees,
  type SeatWorktreeRow,
} from "@/shared/api/tauriCodingSessionWorktrees";

/**
 * Put each verifier's and runner's commit into its own seat's tree, once.
 *
 * The rule, exactly:
 *
 * - A candidate is a governed assignment whose `assigneeRole` is `verifier` or
 *   `runner` **and** which carries a non-empty `baseSha`. A builder is never a
 *   candidate.
 * - A verifier or runner assignment that names **no** revision is not a
 *   candidate either — there is nothing to establish and inventing one would
 *   be a guess — but it is not silent: it is published as the `unnamed` state,
 *   which the row says out loud and offers no retry for. Such assignments were
 *   really published (hive, 2026-09-14) and historical folding keeps them.
 * - Candidates are attempted **in assignment order**, one at a time. The queue
 *   is serial, so two establishments never overlap — for one seat or for any
 *   two.
 * - Each candidate is attempted **at most once per (assignmentId, commit)**
 *   for the life of the mounted surface. A re-render, a refold, or a new
 *   transaction arriving does not re-attempt one already tried; a *new* commit
 *   on the same assignment is a new attempt.
 * - **This computer's durable record is read first.** An attempt recorded for
 *   the *same* commit is dispositive: it is shown, marked as a record rather
 *   than a live check, and nothing is established again. That is what makes a
 *   remount — or an input another surface established — show the answer
 *   instead of an empty row. A record for a *different* commit is no attempt
 *   for this one and says so; the live attempt then proceeds.
 * - **Nothing is retried automatically.** A refusal is a fact about this
 *   computer now; re-running it on a timer would turn one honest sentence into
 *   a loop. {@link CodingSessionAssignmentInputs.retry} is the only way back,
 *   and it bypasses the record so a person's click always reaches the host.
 * - It is **inert off-host**. This computer's record of the trees it cut is
 *   read first; an assignment whose seat has no recorded tree here is disclosed
 *   as `unrecorded_tree` and the command is never called. That is the ordinary
 *   case — most people watching a mission did not hire its seats.
 *
 * Establishing is deliberately **not ordered against the lead's wake**. The
 * wake is published by the hire path and this runs off the mission surface, so
 * a seat can be woken — and can take a turn — before its tree moves. The copy
 * module says so on every established row rather than the two being sequenced
 * here, which no amount of local ordering could actually guarantee.
 */

/** One assignment, as the trigger needs it. */
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
  listSeatWorktrees: (
    sessions: readonly { sessionRef: string }[],
  ) => Promise<SeatWorktreeRow[]>;
  readRecord: (input: {
    assignmentId: string;
  }) => Promise<CodingSessionAssignmentInputRecordRead>;
  establish: (input: {
    assignmentId: string;
    sessionRef: string;
    seatLabel: string;
    commit: string;
    branch: string | null;
  }) => Promise<CodingSessionAssignmentInputOutcome>;
};

const HOST_DEPS: CodingSessionAssignmentInputDeps = {
  listSeatWorktrees: (sessions) =>
    listCodingSessionSeatWorktrees(
      sessions.map((session) => ({
        sessionRef: session.sessionRef,
        sessionSettled: false,
        executionLive: false,
        tipOnRelay: null,
        settledForSecs: null,
      })),
    ),
  readRecord: (input) => readCodingSessionAssignmentInputRecord(input),
  establish: (input) => establishCodingSessionAssignmentInput(input),
};

export type CodingSessionAssignmentInputs = {
  /** assignment event id → what this host has to say about its input. */
  states: ReadonlyMap<string, CodingSessionAssignmentInputState>;
  /** Attempt one assignment again, from a person's own click. */
  retry: (assignmentId: string) => void;
};

type Candidate = {
  assignmentId: string;
  seatLabel: string | null;
  commit: string;
  branch: string | null;
};

function attemptKey(candidate: Candidate): string {
  return `${candidate.assignmentId}@${candidate.commit}`;
}

/**
 * Which assignments this host would establish an input for, in order.
 *
 * Exported so the rule is testable without React: role, a named revision, and
 * nothing else decides candidacy.
 */
export function codingSessionAssignmentInputCandidates(
  assignments: readonly CodingSessionAssignmentInputTarget[],
  resolveSeatLabel: (actor: string) => string | null,
): Candidate[] {
  const seen = new Set<string>();
  const candidates: Candidate[] = [];
  for (const assignment of assignments) {
    const commit = (assignment.baseSha ?? "").trim();
    if (!isCodingSessionInputBoundRole(assignment.assigneeRole)) continue;
    if (commit.length === 0) continue;
    if (seen.has(assignment.assignmentId)) continue;
    seen.add(assignment.assignmentId);
    candidates.push({
      assignmentId: assignment.assignmentId,
      seatLabel:
        assignment.assigneeActor === null
          ? null
          : resolveSeatLabel(assignment.assigneeActor),
      commit,
      branch: assignment.branch ?? null,
    });
  }
  return candidates;
}

/**
 * The role-bound assignments that name no revision at all, in order.
 *
 * Exported beside {@link codingSessionAssignmentInputCandidates} because the
 * two together are the whole partition of the assignments a row may speak
 * about: everything else is a builder, and a builder's row says nothing.
 */
export function codingSessionAssignmentsWithoutInput(
  assignments: readonly CodingSessionAssignmentInputTarget[],
): string[] {
  const seen = new Set<string>();
  const unnamed: string[] = [];
  for (const assignment of assignments) {
    if (!isCodingSessionInputBoundRole(assignment.assigneeRole)) continue;
    if ((assignment.baseSha ?? "").trim().length > 0) continue;
    if (seen.has(assignment.assignmentId)) continue;
    seen.add(assignment.assignmentId);
    unnamed.push(assignment.assignmentId);
  }
  return unnamed;
}

/** The sentence a seat read that could not be made leaves behind. */
const SEAT_READ_FAILED =
  "This computer could not read its record of this session's worktrees.";

export function useCodingSessionAssignmentInputs(input: {
  /** The umbrella's session ref; nothing runs without one. */
  sessionRef: string | null;
  /** False while the mission surface is closed — then nothing runs at all. */
  enabled: boolean;
  assignments: readonly CodingSessionAssignmentInputTarget[];
  /** Actor pubkey → the seat label this host cut the tree under. */
  resolveSeatLabel: (actor: string) => string | null;
  /** Injected only by tests. */
  deps?: CodingSessionAssignmentInputDeps;
}): CodingSessionAssignmentInputs {
  const { enabled, resolveSeatLabel, sessionRef } = input;
  const deps = input.deps ?? HOST_DEPS;
  const [states, setStates] = React.useState<
    ReadonlyMap<string, CodingSessionAssignmentInputState>
  >(() => new Map());

  const attemptedRef = React.useRef(new Set<string>());
  // Attempt keys a person asked for again. A forced key skips the record read:
  // a click that only re-read the record it already showed would do nothing.
  const forcedRef = React.useRef(new Set<string>());
  const pumpingRef = React.useRef(false);
  const mountedRef = React.useRef(true);
  const rowsRef = React.useRef<{
    sessionRef: string;
    rows: SeatWorktreeRow[] | null;
  } | null>(null);
  const depsRef = React.useRef(deps);
  depsRef.current = deps;
  const resolveRef = React.useRef(resolveSeatLabel);
  resolveRef.current = resolveSeatLabel;

  const candidates = React.useMemo(
    () =>
      enabled && sessionRef !== null
        ? codingSessionAssignmentInputCandidates(input.assignments, (actor) =>
            resolveRef.current(actor),
          )
        : [],
    [enabled, input.assignments, sessionRef],
  );
  const candidatesRef = React.useRef(candidates);
  candidatesRef.current = candidates;
  const unnamed = React.useMemo(
    () =>
      enabled ? codingSessionAssignmentsWithoutInput(input.assignments) : [],
    [enabled, input.assignments],
  );

  React.useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  // A new session is a different computer's question: drop the record of what
  // was attempted rather than letting it suppress the new mission's first try.
  // Adjusted during render, not in an effect, so the first render of the new
  // session never shows the previous one's answers even for a frame.
  const [heldSession, setHeldSession] = React.useState(sessionRef);
  if (heldSession !== sessionRef) {
    setHeldSession(sessionRef);
    attemptedRef.current = new Set<string>();
    forcedRef.current = new Set<string>();
    rowsRef.current = null;
    setStates(new Map());
  }

  const publish = React.useCallback(
    (assignmentId: string, state: CodingSessionAssignmentInputState | null) => {
      if (!mountedRef.current) return;
      setStates((previous) => {
        const next = new Map(previous);
        if (state === null) next.delete(assignmentId);
        else next.set(assignmentId, state);
        return next;
      });
    },
    [],
  );

  const seatRows = React.useCallback(
    async (session: string): Promise<SeatWorktreeRow[] | null> => {
      const held = rowsRef.current;
      if (held !== null && held.sessionRef === session) return held.rows;
      const rows = await depsRef.current
        .listSeatWorktrees([{ sessionRef: session }])
        .catch(() => null);
      rowsRef.current = { sessionRef: session, rows };
      return rows;
    },
    [],
  );

  const pump = React.useCallback(async () => {
    if (pumpingRef.current) return;
    pumpingRef.current = true;
    try {
      for (;;) {
        if (!mountedRef.current) return;
        const next = candidatesRef.current.find(
          (candidate) => !attemptedRef.current.has(attemptKey(candidate)),
        );
        if (next === undefined) return;
        const key = attemptKey(next);
        attemptedRef.current.add(key);
        const forced = forcedRef.current.delete(key);
        const session = sessionRef;
        if (session === null) return;
        let stale = false;
        if (!forced) {
          const read = await depsRef.current.readRecord({
            assignmentId: next.assignmentId,
          });
          if (read.kind === "record") {
            const recordedCommit = read.record.commit;
            if (recordedCommit !== null && recordedCommit === next.commit) {
              // Dispositive: this computer already did this, for this commit.
              publish(next.assignmentId, {
                kind: "recorded",
                commit: next.commit,
                inner: codingSessionAssignmentInputStateFromRecord(read.record),
                record: read.record,
              });
              continue;
            }
            // A record for another revision is not an answer about this one.
            publish(next.assignmentId, {
              kind: "stale-record",
              commit: next.commit,
              recordedCommit,
            });
            stale = true;
          }
        }
        if (!mountedRef.current) return;
        // The stale-record sentence stands while the live attempt runs: it is
        // the truer thing to show, and replacing it with `establishing` would
        // hide that there is no answer for this commit yet.
        if (!stale) {
          publish(next.assignmentId, { kind: "pending", commit: next.commit });
        }
        const rows = await seatRows(session);
        if (rows === null) {
          publish(next.assignmentId, {
            kind: "unavailable",
            commit: next.commit,
            message: SEAT_READ_FAILED,
            detail: null,
          });
          continue;
        }
        const tree =
          next.seatLabel === null
            ? undefined
            : rows.find(
                (row) =>
                  row.sessionRef === session &&
                  row.seatLabel === next.seatLabel,
              );
        if (tree === undefined) {
          // Off-host, and said so without calling the command: this computer
          // has no tree of that seat's to move.
          publish(next.assignmentId, {
            kind: "refused",
            commit: next.commit,
            code: "unrecorded_tree",
            message: "No worktree for this seat is recorded on this computer.",
            detail: null,
            changes: null,
          });
          continue;
        }
        const outcome = await depsRef.current.establish({
          assignmentId: next.assignmentId,
          sessionRef: session,
          seatLabel: tree.seatLabel,
          commit: next.commit,
          branch: next.branch,
        });
        publish(
          next.assignmentId,
          codingSessionAssignmentInputState(next.commit, outcome),
        );
      }
    } finally {
      pumpingRef.current = false;
    }
  }, [publish, seatRows, sessionRef]);

  // An assignment that names no commit is stated, not attempted. It needs no
  // session ref and no host at all: the fact is in the signed body.
  React.useEffect(() => {
    if (unnamed.length === 0) return;
    setStates((previous) => {
      let next: Map<string, CodingSessionAssignmentInputState> | null = null;
      for (const assignmentId of unnamed) {
        if (previous.has(assignmentId)) continue;
        next ??= new Map(previous);
        next.set(assignmentId, { kind: "unnamed" });
      }
      return next ?? previous;
    });
  }, [unnamed]);

  React.useEffect(() => {
    if (!enabled || sessionRef === null || candidates.length === 0) return;
    void pump();
  }, [candidates, enabled, pump, sessionRef]);

  const retry = React.useCallback(
    (assignmentId: string) => {
      const candidate = candidatesRef.current.find(
        (entry) => entry.assignmentId === assignmentId,
      );
      if (candidate === undefined) return;
      attemptedRef.current.delete(attemptKey(candidate));
      forcedRef.current.add(attemptKey(candidate));
      // A retry is a person asking this computer to look again, so the cached
      // seat read goes too — the tree may have been cut since.
      rowsRef.current = null;
      publish(assignmentId, null);
      void pump();
    },
    [publish, pump],
  );

  return { states, retry };
}
