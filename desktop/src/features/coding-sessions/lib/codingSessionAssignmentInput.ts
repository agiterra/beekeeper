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
 *    lands in its tree. Every sentence this file produces says so.
 */

/** The native command that moves a seat's tree onto its assignment's commit. */
export const CODING_SESSION_ESTABLISH_ASSIGNMENT_INPUT_COMMAND =
  "coding_session_establish_assignment_input";

/** The read-only companion: the last attempt this host recorded. */
export const CODING_SESSION_ASSIGNMENT_INPUT_RECORD_COMMAND =
  "coding_session_assignment_input_record";

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
  recordedAt: number;
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
    const shaped = asRecord(answer);
    if (shaped === null || asString(shaped.outcome) === null) {
      return { kind: "none" };
    }
    return {
      kind: "record",
      record: {
        assignmentId: asString(shaped.assignmentId) ?? input.assignmentId,
        sessionRef: asString(shaped.sessionRef),
        seatLabel: asString(shaped.seatLabel),
        commit: asString(shaped.commit),
        branch: asString(shaped.branch),
        path: asString(shaped.path),
        remote: asString(shaped.remote),
        outcome: asString(shaped.outcome) ?? "",
        message: asString(shaped.message),
        changes: changeCount(shaped.changes),
        recordedAt:
          typeof shaped.recordedAt === "number" ? shaped.recordedAt : 0,
      },
    };
  } catch (error) {
    return { kind: "unavailable", message: messageOf(error) };
  }
}
