import { toast } from "sonner";

import type { CreateProjectContainerOutcome } from "../useCreateProjectContainer";
import { describeAgentsSetup } from "./projectAgentsInit";
import { storeProjectSetupOutcome } from "./projectSetupOutcome";

/**
 * Say what creating a project actually produced (spec § 4.11): the head is
 * published either way; the repositories' fate is the host's own verdict,
 * printed as is. A gap is a warning that names Finish setup, never an
 * error that hides a project which now exists.
 *
 * The toast does not expire, and the same outcome is written down for the
 * new project's Overview: creation does seven things, and on 2026-09-20 its
 * report vanished before the operator could read it (ledger 207(5)). A
 * result you cannot read is a result you cannot trust.
 */
export function toastCreateProjectOutcome(
  outcome: CreateProjectContainerOutcome,
): void {
  const name = outcome.project.name;
  storeProjectSetupOutcome({
    projectRef: outcome.project.address,
    projectName: name,
    at: new Date().toISOString(),
    result: outcome.repositories,
    error: outcome.repositoriesError,
    verify: outcome.verify,
    verifyError: outcome.verifyError,
  });
  // Dismissed by the person, never by a timer.
  const persist = { duration: Number.POSITIVE_INFINITY } as const;
  if (outcome.repositories?.complete) {
    toast.success(
      `Project "${name}" created. ${describeAgentsSetup(outcome.repositories)}`,
      persist,
    );
    return;
  }
  if (outcome.repositories) {
    toast.warning(
      `Project "${name}" created. ${describeAgentsSetup(outcome.repositories)}`,
      persist,
    );
    return;
  }
  toast.warning(
    `Project "${name}" created, but its repositories were not: ${outcome.repositoriesError ?? "unknown"}. Finish setup from Project settings → Packs.`,
    persist,
  );
}
