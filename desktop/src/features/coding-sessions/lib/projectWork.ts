/**
 * Presentation for the native work-coverage projection.
 *
 * **This file contains no fold.** Every status, reason and commit it renders
 * comes from `buzz-core`; what lives here is the wording of a *label*, the
 * ordering of rows, and the one rule the projection deliberately does not
 * take a position on: which single next step, if any, a reader should be
 * offered. Anything that decides whether a criterion is covered belongs in
 * `crates/beekeeper-core/src/project_work_fold.rs` and nowhere else.
 */
import type {
  ProjectWorkCoverage,
  ProjectWorkCriterion,
  ProjectWorkDeclaration,
  ProjectWorkResponse,
} from "@/shared/api/tauriProjectWork";

/** `abababab…` — twelve characters, the contract's own abbreviation. */
export function shortCommit(commit: string): string {
  return commit.length > 12 ? `${commit.slice(0, 12)}…` : commit;
}

/** `plans/kettle.md @ ababab…` with the repository that holds it. */
export function planLabel(declaration: ProjectWorkDeclaration): string {
  const repository = declaration.planRef.repository.slice(
    declaration.planRef.repository.lastIndexOf(":") + 1,
  );
  return `${declaration.planRef.path} @ ${shortCommit(declaration.planRef.commit)} in ${repository}`;
}

/** The four declaration states, each in its own words — never merged. */
export function declarationStateLabel(
  declaration: ProjectWorkDeclaration,
): string {
  switch (declaration.state) {
    case "head":
      return "in force";
    case "superseded":
      return `superseded by ${declaration.supersededBy.map((id) => id.slice(0, 8)).join(", ") || "a later declaration"}`;
    case "stale":
      return "pinned to an earlier goal";
    case "conflict":
      return "forked: more than one declaration is current";
  }
}

/** Who owes a criterion, from the assignments bound to it. */
export function owedBy(
  criterion: ProjectWorkCriterion,
  resolveActorName?: (pubkey: string) => string,
): string {
  if (criterion.assignmentRefs.length === 0) {
    // The contract is explicit that this is "nobody has been assigned it
    // yet", not "no evidence has arrived".
    return "nobody is assigned this yet";
  }
  const names = criterion.assignmentRefs.map(
    (ref) => resolveActorName?.(ref) ?? `${ref.slice(0, 8)}…`,
  );
  return `assigned: ${names.join(", ")}`;
}

/**
 * What a criterion's status means, in words, without restating its reason.
 *
 * The fold writes the *reason*; this writes the *state*, so a row can show
 * both and a reader never has to infer either.
 */
export function criterionStatusLabel(status: string): string {
  switch (status) {
    case "covered":
      return "covered";
    case "open":
      return "open";
    case "stale":
      return "stale — bound to another revision of this contract";
    case "unknown":
      return "unknown — an input could not be read";
    default:
      return status;
  }
}

/** One actionable next step, and the fact that would release it. */
export type ProjectWorkNextStep = {
  text: string;
  /** The fact whose arrival makes this step unnecessary. */
  releasedBy: string;
};

/**
 * At most one next step.
 *
 * Deliberately at most one: a surface that lists five things to do is a
 * surface nobody acts on. The order is the order in which the obstacles
 * actually block each other — a fork must be resolved before any of its
 * declarations can complete; a contract nobody can read cannot be worked
 * against; only then does an open criterion matter.
 *
 * Returns `null` when nothing is owed, including when everything is
 * `unknown` for a reason the reader cannot act on from here.
 */
export function nextStep(
  response: Pick<ProjectWorkResponse, "coverage" | "unreadablePlans">,
): ProjectWorkNextStep | null {
  const { coverage } = response;
  const conflict = coverage.conflicts[0];
  if (conflict) {
    return {
      text: `Resolve the fork on ${conflict.workId.slice(0, 8)}… with one declaration naming every current head.`,
      releasedBy: "a work.declared naming all current heads",
    };
  }
  const unreadable = response.unreadablePlans[0];
  if (unreadable) {
    return {
      text: `Make ${unreadable.path} readable at ${shortCommit(unreadable.commit)} on this computer — ${unreadable.reason}`,
      releasedBy: "the plan blob at its pinned commit",
    };
  }
  const head = coverage.declarations.find(
    (declaration) => declaration.state === "head" && declaration.planResolved,
  );
  if (!head) return null;
  if (head.coverageComplete) return null;
  const open = head.criteria.find(
    (criterion) => criterion.status === "open" || criterion.status === "stale",
  );
  if (open) {
    return {
      text: `${open.criterionId} is ${open.status}${open.reason ? ` — ${open.reason}` : ""}.`,
      releasedBy: `evidence bound to ${open.criterionId} under this declaration`,
    };
  }
  if (head.coverageReason) {
    return {
      text: head.coverageReason,
      releasedBy: head.coverageReasonCode ?? "the fold's next read",
    };
  }
  return null;
}

/**
 * The plan-drift sentence for one declaration, and the command that settles
 * it — or `null` when nothing moved, nothing is known, or the move was
 * already adopted.
 *
 * **Wording is the contract's** (A10; README § (c) "What this fact observes,
 * and what it cannot"): relay ref state names a *branch tip*, so this says
 * the agents repository has moved on since the pinned plan commit. It must
 * never say the plan file changed — nothing the relay publishes is
 * path-scoped — and it never claims the work is stale: the plan at the pinned
 * commit is still what this work is judged against.
 */
export function planDriftNotice(
  declaration: ProjectWorkDeclaration,
  scope?: { channelRef: string; sessionRef: string },
): { text: string; command: string } | null {
  const drift = declaration.planDrift;
  if (drift.state !== "drifted" || !drift.currentCommit) return null;
  const declared = shortCommit(drift.declaredCommit);
  const current = shortCommit(drift.currentCommit);
  const channel = scope?.channelRef ?? "<channel uuid>";
  const session = scope?.sessionRef ?? "<session uuid>";
  return {
    text: `agents repo main moved on: ${declared}→${current} — the plan this work is judged against did not change; the agents repository's main has moved past the commit it pinned, and this work is still judged against the plan at ${declared}.`,
    command: `bee sessions work adopt --plan ${declaration.planRef.path} --commit ${drift.currentCommit} --agents-repo <dir> --channel ${channel} --session-ref ${session} --work-id ${declaration.workId} --supersedes ${declaration.declarationRef}`,
  };
}

/** Declarations a reader should see, current contracts first. */
export function orderedDeclarations(
  coverage: ProjectWorkCoverage,
): readonly ProjectWorkDeclaration[] {
  const rank: Record<string, number> = {
    conflict: 0,
    head: 1,
    stale: 2,
    superseded: 3,
  };
  return [...coverage.declarations].sort(
    (a, b) => (rank[a.state] ?? 9) - (rank[b.state] ?? 9),
  );
}
