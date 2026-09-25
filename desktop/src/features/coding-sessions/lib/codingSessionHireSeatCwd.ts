/**
 * Where a hired seat starts, and what happens to its tree when it cannot.
 *
 * Control run 7 (2026-09-24): the hire cut and recorded the builder's
 * worktree, staged its path as a one-shot hint, and the builder still started
 * in the project's own checkout — the host's hint map was full and evicted the
 * hint it had just staged, and the provider fell through to the project
 * default. The verifier hire after it was refused `SEAT_CWD_SHARED` and left
 * its freshly cut tree behind.
 *
 * Two rules live here. The seat's hint is staged by the host *from the seat's
 * own worktree record* (`stage_coding_session_seat_create_hint`), which
 * refuses by name — `SEAT_CWD_UNRECORDED`, `SEAT_CWD_PROJECT_ROOT`,
 * `SEAT_CWD_SHARED` — rather than falling back; and a hire that ends refused,
 * before or after its create was published, removes the tree it cut.
 */
import type {
  CodingSessionHireDeps,
  UseCodingSessionHireInput,
} from "../hooks/useCodingSessionHire";
import { refuseCodingSessionHireForSeatingFailure } from "./codingSessionHireDisclosure";
import type { CodingSessionHireSeatPlan } from "./codingSessionHireSeat";
import type { CodingSessionHireRequest } from "./codingSessionHireWire";

/** The code the host's error text leads with, or null when it names none. */
export function codingSessionSeatCwdRefusalCode(text: string): string | null {
  return /^(SEAT_CWD_[A-Z_]+):/.exec(text.trim())?.[1] ?? null;
}

function errorText(error: unknown): string {
  return error instanceof Error && error.message.trim()
    ? error.message.trim()
    : String(error);
}

/**
 * Stage the seat's create hint at its own recorded worktree, or refuse the
 * hire and remove the tree this hire cut.
 *
 * Resolves null when staged, or the refusal code the outcome records — the
 * host's `SEAT_CWD_*` code when it named one. Published as
 * `HIRE_SEAT_STAGING_FAILED` with the host's sentence verbatim, the same way
 * every other post-cut staging failure is (ledger 169).
 */
export async function stageHiredSeatWorkdirOrRefuse(
  request: CodingSessionHireRequest,
  plan: CodingSessionHireSeatPlan,
  worktreePath: string,
  input: UseCodingSessionHireInput,
  deps: CodingSessionHireDeps,
): Promise<string | null> {
  try {
    await deps.stageSeatCreateHint({
      commandId: plan.commandId,
      sessionRef: plan.sessionRef,
      seatLabel: plan.seatLabel,
      projectRef: plan.projectRef,
    });
    return null;
  } catch (error: unknown) {
    const failure = errorText(error);
    await refuseCodingSessionHireForSeatingFailure(
      request,
      {
        failure,
        sessionRef: plan.sessionRef,
        seatLabel: plan.seatLabel,
        worktreePath,
      },
      input,
      deps,
    );
    return (
      codingSessionSeatCwdRefusalCode(failure) ?? "HIRE_SEAT_STAGING_FAILED"
    );
  }
}

/**
 * Remove the tree cut for a seat whose create the provider refused.
 *
 * The provider refuses before it spawns anything, so the tree is exactly as
 * the hire cut it; the host's own prune removes it (never `--force`, so a tree
 * somebody did write into stays). Resolves to the sentence the grant failure
 * appends, whether or not the prune worked.
 */
export async function disposeRefusedSeatWorktree(
  plan: Pick<CodingSessionHireSeatPlan, "sessionRef" | "seatLabel">,
  deps: Pick<CodingSessionHireDeps, "disposeSeatWorktree">,
): Promise<string> {
  try {
    return await deps.disposeSeatWorktree({
      sessionRef: plan.sessionRef,
      seatLabel: plan.seatLabel,
    });
  } catch (error: unknown) {
    return `the worktree cut for ${plan.seatLabel} could not be removed: ${errorText(error)}`;
  }
}
