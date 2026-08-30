import type { TeamReadinessResponse } from "@/shared/api/tauriTeamReadiness";

export type TeamReadinessPrepareStepId =
  | "install_roles"
  | "provision_provider"
  | "start_provider"
  | "reread_readiness";
export type TeamReadinessPrepareStep = {
  id: TeamReadinessPrepareStepId;
  label: string;
  state: "pending" | "running" | "done" | "failed";
  detail?: string;
};

export type TeamReadinessPrepareDependencies = {
  installRoles: () => Promise<void>;
  provisionProvider: () => Promise<void>;
  startProvider: () => Promise<void>;
  rereadReadiness: () => Promise<TeamReadinessResponse>;
};

const INITIAL_STEPS: TeamReadinessPrepareStep[] = [
  {
    id: "install_roles",
    label: "Install or refresh discovered role packs",
    state: "pending",
  },
  {
    id: "provision_provider",
    label: "Provision the provider identity",
    state: "pending",
  },
  { id: "start_provider", label: "Start the provider", state: "pending" },
  { id: "reread_readiness", label: "Re-read Team Readiness", state: "pending" },
];

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Run named existing mutations in order and always finish with a fresh read. */
export async function prepareProjectForTeams(input: {
  dependencies: TeamReadinessPrepareDependencies;
  onSteps?: (steps: readonly TeamReadinessPrepareStep[]) => void;
}): Promise<{
  steps: TeamReadinessPrepareStep[];
  readiness: TeamReadinessResponse | null;
  error: string | null;
}> {
  const steps = INITIAL_STEPS.map((step) => ({ ...step }));
  const publish = () => input.onSteps?.(steps.map((step) => ({ ...step })));
  const run = async (
    id: Exclude<TeamReadinessPrepareStepId, "reread_readiness">,
    action: () => Promise<void>,
  ) => {
    const step = steps.find((entry) => entry.id === id);
    if (!step) return;
    step.state = "running";
    publish();
    await action();
    step.state = "done";
    publish();
  };

  let error: string | null = null;
  let readiness: TeamReadinessResponse | null = null;
  try {
    await run("install_roles", input.dependencies.installRoles);
    await run("provision_provider", input.dependencies.provisionProvider);
    await run("start_provider", input.dependencies.startProvider);
  } catch (cause) {
    error = errorMessage(cause);
    const running = steps.find((step) => step.state === "running");
    if (running) {
      running.state = "failed";
      running.detail = error;
    }
    publish();
  } finally {
    const reread = steps.find((step) => step.id === "reread_readiness");
    if (reread) {
      reread.state = "running";
      publish();
      try {
        readiness = await input.dependencies.rereadReadiness();
        reread.state = "done";
      } catch (cause) {
        const detail = errorMessage(cause);
        reread.state = "failed";
        reread.detail = detail;
        error = error
          ? `${error} Readiness re-check failed: ${detail}`
          : detail;
      }
      publish();
    }
  }
  return { steps, readiness, error };
}
