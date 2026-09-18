import { toast } from "sonner";

import type { CreateProjectContainerOutcome } from "../useCreateProjectContainer";
import { describeAgentsSetup } from "./projectAgentsInit";

/**
 * Say what creating a project actually produced (spec § 4.11): the head is
 * published either way; the repositories' fate is the host's own verdict,
 * printed as is. A gap is a warning that names Finish setup, never an
 * error that hides a project which now exists.
 */
export function toastCreateProjectOutcome(
  outcome: CreateProjectContainerOutcome,
): void {
  const name = outcome.project.name;
  if (outcome.repositories?.complete) {
    toast.success(
      `Project "${name}" created. ${describeAgentsSetup(outcome.repositories)}`,
    );
    return;
  }
  if (outcome.repositories) {
    toast.warning(
      `Project "${name}" created. ${describeAgentsSetup(outcome.repositories)}`,
    );
    return;
  }
  toast.warning(
    `Project "${name}" created, but its repositories were not: ${outcome.repositoriesError ?? "unknown"}. Finish setup from Project settings → Packs.`,
  );
}
