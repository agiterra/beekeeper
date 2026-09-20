import { invokeTauri } from "@/shared/api/tauri";

/**
 * The revision a verifier or a runner is asked to work from, put into its
 * seat's tree by this computer.
 *
 * A hired seat's worktree is cut from the trunk (`useCodingSessionHire.ts`
 * passes `source: null`), so a seat asked to verify commit X starts on `main`
 * and verifies whatever `main` happens to hold. The native command below moves
 * that seat's recorded tree onto the commit its assignment names.
 *
 * Three things this layer will not do:
 *
 * 1. **It never decides.** Every refusal code is the host's; this file maps a
 *    code to a sentence and nothing else. A code this build has never heard of
 *    stays a refusal, named by its own token.
 * 2. **It never reads a thrown error as success.** A build where the command
 *    is not registered throws something that carries no contract `code`, and
 *    that surfaces as {@link CodingSessionAssignmentInputOutcome} `unavailable`
 *    — a disclosure, never an establishment.
 * 3. **It is not ordered against the wake.** Establishing the input and waking
 *    the seat are independent; the seat may take a turn before the commit
 *    lands in its tree. What makes that safe is the provider's fence, which
 *    refuses a verifier or runner turn whose tree does not hold the
 *    assignment's commit — and needs a re-issued assignment afterwards. Every
 *    sentence this file produces says so.
 *
 * **Who decides when.** Nothing on this side does, any more. The host owns a
 * durable queue (`desktop/src-tauri/src/coding_sessions/assignment_establishment.rs`):
 * a surface hands over the assignments it folded, the host records an intent,
 * establishes it, and finishes anything a quit left pending at the next
 * launch. Before that, a React effect in the mission panel held the whole
 * sequence, so closing the panel decided whether the work happened at all
 * (ledger 185).
 */

/** The native command that moves a seat's tree onto its assignment's commit. */
export const CODING_SESSION_ESTABLISH_ASSIGNMENT_INPUT_COMMAND =
  "coding_session_establish_assignment_input";

/** The read-only companion: the last attempt this host recorded. */
export const CODING_SESSION_ASSIGNMENT_INPUT_RECORD_COMMAND =
  "coding_session_assignment_input_record";

/**
 * Hand the host the assignments a surface folded, and read back what it did.
 *
 * The only call a surface makes on this path now: the host queues, establishes
 * and records. It is safe to call again with the same assignments — a record
 * that already names the same commit is dispositive, refusal included, so
 * nothing loops.
 */
export const CODING_SESSION_OBSERVE_ASSIGNMENT_INPUTS_COMMAND =
  "coding_session_observe_assignment_inputs";

/** A person's click: put one settled assignment back in the host's queue. */
export const CODING_SESSION_REQUEUE_ASSIGNMENT_INPUT_COMMAND =
  "coding_session_requeue_assignment_input";

/**
 * The outcome words the host writes that are not an attempt's result.
 *
 * `intended` is queued and untried, `establishing` is an attempt the host
 * started, and `establish_abandoned` is two started attempts that never
 * finished — the bounded end of a replay, not a git refusal. They share the
 * record's `outcome` field with the refusal codes on purpose: one field, one
 * answer, and a build that has not heard of a word says so by name.
 */
export const CODING_SESSION_ASSIGNMENT_INPUT_QUEUED = "intended";
export const CODING_SESSION_ASSIGNMENT_INPUT_ESTABLISHING = "establishing";
export const CODING_SESSION_ASSIGNMENT_INPUT_ABANDONED = "establish_abandoned";

/**
 * Why the host has, or has not, a record for one observed assignment.
 *
 * Mirrors `CodingSessionAssignmentInputDisposition` in
 * `assignment_establishment.rs`; an unrecognised word is read as
 * `unreadable` rather than as any of the five, because guessing which one it
 * meant is exactly how a surface comes to show "nothing to do" for work that
 * was never done.
 */
export const CODING_SESSION_ASSIGNMENT_INPUT_DISPOSITIONS = [
  "not_required",
  "unnamed",
  "invalid",
  "off_host",
  "recorded",
] as const;

export type CodingSessionAssignmentInputDisposition =
  (typeof CODING_SESSION_ASSIGNMENT_INPUT_DISPOSITIONS)[number];

/** One assignment as the host takes it: signed fields plus this host's seat. */
export type ObservedCodingSessionAssignment = {
  assignmentId: string;
  sessionRef: string;
  /** The seat label this host cut the tree under, when one resolves. */
  seatLabel: string | null;
  assigneeRole: string | null;
  baseSha: string | null;
  branch?: string | null;
};

/** The host's answer for one observed assignment. */
export type CodingSessionAssignmentInputStatus = {
  assignmentId: string;
  disposition: CodingSessionAssignmentInputDisposition | "unreadable";
  record: CodingSessionAssignmentInputRecord | null;
};

/**
 * The roles whose assignment carries a `baseSha` the seat must start from.
 *
 * A builder's assignment may carry one and it is not established: a builder
 * writes the revision rather than measuring it.
 */
export const CODING_SESSION_INPUT_BOUND_ROLES = ["verifier", "runner"] as const;

export type CodingSessionInputBoundRole =
  (typeof CODING_SESSION_INPUT_BOUND_ROLES)[number];

/** True when an assignment's `assigneeRole` is one this host establishes for. */
export function isCodingSessionInputBoundRole(
  role: string | null | undefined,
): role is CodingSessionInputBoundRole {
  return (CODING_SESSION_INPUT_BOUND_ROLES as readonly string[]).includes(
    (role ?? "").trim().toLowerCase(),
  );
}

/** Every refusal code the native command is contracted to throw. */
export const CODING_SESSION_ASSIGNMENT_INPUT_REFUSALS = [
  "unrecorded_tree",
  "missing_tree",
  "dirty_tree",
  "no_remote",
  "ambiguous_remote",
  "fetch_failed",
  "unknown_commit",
  "checkout_failed",
  "invalid_input",
] as const;

export type CodingSessionAssignmentInputRefusal =
  (typeof CODING_SESSION_ASSIGNMENT_INPUT_REFUSALS)[number];

export function isCodingSessionAssignmentInputRefusal(
  value: unknown,
): value is CodingSessionAssignmentInputRefusal {
  return (
    CODING_SESSION_ASSIGNMENT_INPUT_REFUSALS as readonly unknown[]
  ).includes(value);
}

/** What the command answers when the seat's tree now holds the commit. */
export type CodingSessionAssignmentInputEstablished = {
  /** The seat's worktree directory. */
  path: string;
  /** The branch the tree is on after the move. */
  branch: string;
  /** The commit the tree now holds — the host's answer, not the request. */
  commit: string;
  /**
   * The remote the commit was fetched from, or `null` when the tree already
   * held the commit and nothing had to be fetched. A tree with no remote
   * configured can still establish a commit it already has, so an absent
   * remote is an ordinary success, not a missing field.
   */
  remote: string | null;
  /** True when the tree already held the commit and nothing moved. */
  alreadyCurrent: boolean;
};

/** One attempt, as this host recorded it. */
export type CodingSessionAssignmentInputRecord = {
  assignmentId: string;
  sessionRef: string | null;
  seatLabel: string | null;
  commit: string | null;
  branch: string | null;
  path: string | null;
  remote: string | null;
  /** `established`, `already_current`, or one of the refusal codes. */
  outcome: string;
  message?: string | null;
  /** Uncommitted-change count on a `dirty_tree` record, when the host has one. */
  changes: number | null;
  /**
   * Attempts the host has *started* for this assignment.
   *
   * Counted before the git work, which is the only count that can bound a
   * replay across a quit. Absent on a record written before the host counted.
   */
  attempts: number;
  /**
   * When the host wrote the record, ISO-8601 — the host's own string.
   *
   * A string, not a number: `recordedAt` is `now_iso()` on the Rust side and
   * always has been, so the `number` this field used to be parsed into was
   * `0` for every record ever written. Nothing rendered it, so nothing lied;
   * it is a string now so that nothing can start to.
   */
  recordedAt: string | null;
};

/**
 * What one establishment attempt came to.
 *
 * `unavailable` is its own arm on purpose. A build without the command
 * registered, a host that went away mid-call, or a response this reader does
 * not recognise all land here — never on `established`, and never silently on
 * a refusal code the host did not actually give.
 */
export type CodingSessionAssignmentInputOutcome =
  | {
      readonly kind: "established";
      readonly result: CodingSessionAssignmentInputEstablished;
    }
  | {
      readonly kind: "refused";
      readonly code: CodingSessionAssignmentInputRefusal | string;
      /** The host's own message, kept for the disclosure's raw block. */
      readonly message: string;
      readonly detail: string | null;
      /** Uncommitted-change count, when the host named one. */
      readonly changes: number | null;
    }
  | {
      readonly kind: "unavailable";
      readonly message: string;
      readonly detail: string | null;
    };

/** The read-only record read, with the same honesty about an absent command. */
export type CodingSessionAssignmentInputRecordRead =
  | {
      readonly kind: "record";
      readonly record: CodingSessionAssignmentInputRecord;
    }
  | { readonly kind: "none" }
  | { readonly kind: "unavailable"; readonly message: string };

function asRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null
    ? (value as Record<string, unknown>)
    : null;
}

function asString(value: unknown): string | null {
  return typeof value === "string" && value.length > 0 ? value : null;
}

/**
 * Find the contract error inside whatever the boundary threw.
 *
 * `invokeTauri` wraps a non-`Error` rejection in a `TauriInvokeError` whose
 * `payload` is the original value, so the contract object may be the throw
 * itself, the wrapper's payload, or a JSON string in either place.
 */
function contractErrorOf(error: unknown): Record<string, unknown> | null {
  const candidates: unknown[] = [error];
  const wrapper = asRecord(error);
  if (wrapper && "payload" in wrapper) candidates.push(wrapper.payload);
  if (wrapper && "cause" in wrapper) candidates.push(wrapper.cause);
  for (const candidate of candidates) {
    const shaped = asRecord(candidate);
    if (shaped && isCodingSessionAssignmentInputRefusal(shaped.code)) {
      return shaped;
    }
  }
  return null;
}

function messageOf(error: unknown): string {
  if (error instanceof Error) return error.message;
  const shaped = asRecord(error);
  const message = shaped ? asString(shaped.message) : null;
  return message ?? String(error);
}

/**
 * How many uncommitted changes the host counted, when it counted any.
 *
 * The count is the host's own `changes` field on the refusal
 * (`EstablishAssignmentInputError`, `changes: Option<u32>`), never recounted
 * and never read back out of its sentence: this side has not seen the tree,
 * and a sentence is not a contract. Absent means the row says "uncommitted
 * changes" without a number rather than inventing one.
 */
function changeCount(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

/**
 * Turn a thrown value into an outcome.
 *
 * Exported because this mapping — contract refusal versus "this build cannot
 * answer" — is the whole honesty of the client, and it is tested directly.
 */
export function decodeCodingSessionAssignmentInputError(
  error: unknown,
): CodingSessionAssignmentInputOutcome {
  const contract = contractErrorOf(error);
  if (contract === null) {
    return {
      kind: "unavailable",
      message: messageOf(error),
      detail: null,
    };
  }
  const message = asString(contract.message) ?? String(contract.code);
  return {
    kind: "refused",
    code: contract.code as CodingSessionAssignmentInputRefusal,
    message,
    detail: asString(contract.detail),
    changes: changeCount(contract.changes),
  };
}

function decodeEstablished(
  value: unknown,
): CodingSessionAssignmentInputOutcome {
  const shaped = asRecord(value);
  const path = shaped ? asString(shaped.path) : null;
  const commit = shaped ? asString(shaped.commit) : null;
  if (shaped === null || path === null || commit === null) {
    return {
      kind: "unavailable",
      message:
        "This computer answered a verification-input shape this build does not recognise.",
      detail: null,
    };
  }
  return {
    kind: "established",
    result: {
      path,
      commit,
      branch: asString(shaped.branch) ?? "",
      remote: asString(shaped.remote),
      alreadyCurrent: shaped.alreadyCurrent === true,
    },
  };
}

/** What the caller asks this host to establish. */
export type CodingSessionAssignmentInputRequest = {
  assignmentId: string;
  sessionRef?: string | null;
  seatLabel?: string | null;
  commit?: string | null;
  branch?: string | null;
};

/**
 * Put the assignment's commit into its seat's tree, and say what happened.
 *
 * Never throws: every failure is an outcome a surface can state. Nothing here
 * retries — a refusal is a fact about this computer right now, and re-running
 * it on a timer would turn one honest sentence into a loop.
 */
export async function establishCodingSessionAssignmentInput(
  request: CodingSessionAssignmentInputRequest,
): Promise<CodingSessionAssignmentInputOutcome> {
  try {
    return decodeEstablished(
      await invokeTauri(CODING_SESSION_ESTABLISH_ASSIGNMENT_INPUT_COMMAND, {
        assignmentId: request.assignmentId,
        sessionRef: request.sessionRef ?? null,
        seatLabel: request.seatLabel ?? null,
        commit: request.commit ?? null,
        branch: request.branch ?? null,
      }),
    );
  } catch (error) {
    return decodeCodingSessionAssignmentInputError(error);
  }
}

/**
 * The last attempt this host recorded for one assignment, or that there is
 * none — and `unavailable` when the read itself could not be made, which is
 * never rendered as "no attempt".
 */
export async function readCodingSessionAssignmentInputRecord(input: {
  assignmentId: string;
}): Promise<CodingSessionAssignmentInputRecordRead> {
  try {
    const answer = await invokeTauri<unknown>(
      CODING_SESSION_ASSIGNMENT_INPUT_RECORD_COMMAND,
      { assignmentId: input.assignmentId },
    );
    const record = decodeCodingSessionAssignmentInputRecord(
      answer,
      input.assignmentId,
    );
    return record === null ? { kind: "none" } : { kind: "record", record };
  } catch (error) {
    return { kind: "unavailable", message: messageOf(error) };
  }
}

/**
 * Read one durable record, or `null` when the value is not one.
 *
 * Exported because two commands answer with the same record shape and a
 * second reader of it would drift — which is how a field like `attempts`
 * comes to be read on one path and dropped on the other.
 */
export function decodeCodingSessionAssignmentInputRecord(
  value: unknown,
  fallbackAssignmentId: string,
): CodingSessionAssignmentInputRecord | null {
  const shaped = asRecord(value);
  const outcome = shaped ? asString(shaped.outcome) : null;
  if (shaped === null || outcome === null) return null;
  return {
    assignmentId: asString(shaped.assignmentId) ?? fallbackAssignmentId,
    sessionRef: asString(shaped.sessionRef),
    seatLabel: asString(shaped.seatLabel),
    commit: asString(shaped.commit),
    branch: asString(shaped.branch),
    path: asString(shaped.path),
    remote: asString(shaped.remote),
    outcome,
    message: asString(shaped.message),
    changes: changeCount(shaped.changes),
    attempts: changeCount(shaped.attempts) ?? 0,
    recordedAt: asString(shaped.recordedAt),
  };
}

function decodeStatus(
  value: unknown,
  fallbackAssignmentId: string,
): CodingSessionAssignmentInputStatus {
  const shaped = asRecord(value);
  const word = shaped ? asString(shaped.disposition) : null;
  const disposition = (
    CODING_SESSION_ASSIGNMENT_INPUT_DISPOSITIONS as readonly string[]
  ).includes(word ?? "")
    ? (word as CodingSessionAssignmentInputDisposition)
    : "unreadable";
  return {
    assignmentId: asString(shaped?.assignmentId) ?? fallbackAssignmentId,
    disposition,
    record:
      shaped === undefined || shaped === null
        ? null
        : decodeCodingSessionAssignmentInputRecord(
            shaped.record,
            asString(shaped.assignmentId) ?? fallbackAssignmentId,
          ),
  };
}

/**
 * Hand the host every assignment this surface folded; read back what it did.
 *
 * Never throws. A build without the command registered, or a host that went
 * away, answers `unavailable` for the whole batch — which a row states rather
 * than showing as "nothing to establish".
 */
export async function observeCodingSessionAssignmentInputs(
  assignments: readonly ObservedCodingSessionAssignment[],
): Promise<
  | { kind: "statuses"; statuses: CodingSessionAssignmentInputStatus[] }
  | { kind: "unavailable"; message: string }
> {
  try {
    const answer = await invokeTauri<unknown>(
      CODING_SESSION_OBSERVE_ASSIGNMENT_INPUTS_COMMAND,
      {
        assignments: assignments.map((assignment) => ({
          assignmentId: assignment.assignmentId,
          sessionRef: assignment.sessionRef,
          seatLabel: assignment.seatLabel,
          assigneeRole: assignment.assigneeRole,
          baseSha: assignment.baseSha,
          branch: assignment.branch ?? null,
        })),
      },
    );
    if (!Array.isArray(answer)) {
      return {
        kind: "unavailable",
        message:
          "This computer answered a verification-input shape this build does not recognise.",
      };
    }
    return {
      kind: "statuses",
      statuses: answer.map((entry, index) =>
        decodeStatus(entry, assignments[index]?.assignmentId ?? ""),
      ),
    };
  } catch (error) {
    return { kind: "unavailable", message: messageOf(error) };
  }
}

/**
 * Ask the host to establish one assignment's input again.
 *
 * `none` when this host holds no record of that assignment — a truer answer
 * than an empty success, and the one a row shows when a person clicks on
 * somebody else's seat.
 */
export async function requeueCodingSessionAssignmentInput(input: {
  assignmentId: string;
}): Promise<
  | { kind: "status"; status: CodingSessionAssignmentInputStatus }
  | { kind: "none" }
  | { kind: "unavailable"; message: string }
> {
  try {
    const answer = await invokeTauri<unknown>(
      CODING_SESSION_REQUEUE_ASSIGNMENT_INPUT_COMMAND,
      { assignmentId: input.assignmentId },
    );
    if (asRecord(answer) === null) return { kind: "none" };
    return { kind: "status", status: decodeStatus(answer, input.assignmentId) };
  } catch (error) {
    return { kind: "unavailable", message: messageOf(error) };
  }
}
