import type { TeamReadinessResponse } from "@/shared/api/tauriTeamReadiness";
import type { InstalledCrewRole } from "@/shared/api/tauriTeams";

export type TeamReadinessPrepareStepId =
  | "install_roles"
  | "start_roles"
  | "provision_provider"
  | "start_provider"
  | "refresh_runtime"
  | "reread_readiness";
export type TeamReadinessPrepareStep = {
  id: TeamReadinessPrepareStepId;
  label: string;
  state: "pending" | "running" | "done" | "failed";
  detail?: string;
};

export type TeamReadinessPrepareDependencies = {
  installRoles: () => Promise<void>;
  startRoles: () => Promise<void>;
  provisionProvider: () => Promise<void>;
  startProvider: () => Promise<void>;
  refreshRuntime: () => Promise<void>;
  rereadReadiness: () => Promise<TeamReadinessResponse>;
};

const INITIAL_STEPS: TeamReadinessPrepareStep[] = [
  {
    id: "install_roles",
    label: "Install or refresh discovered role packs",
    state: "pending",
  },
  {
    id: "start_roles",
    label: "Start the selected managed role identities",
    state: "pending",
  },
  {
    id: "provision_provider",
    label: "Provision the provider identity",
    state: "pending",
  },
  { id: "start_provider", label: "Start the provider", state: "pending" },
  {
    id: "refresh_runtime",
    label: "Refresh provider runtime and sign-in state",
    state: "pending",
  },
  { id: "reread_readiness", label: "Re-read Team Readiness", state: "pending" },
];

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Start exactly the installed identities required by this launch, failing closed. */
export async function startSelectedInstalledRoleIdentities(input: {
  installed: readonly InstalledCrewRole[];
  selectedRoles: readonly string[];
  start: (pubkey: string) => Promise<unknown>;
}): Promise<void> {
  const roles = [
    ...new Set(
      input.selectedRoles
        .map((role) => role.trim().toLowerCase())
        .filter(Boolean),
    ),
  ].sort();
  for (const role of roles) {
    const matches = input.installed.filter(
      (entry) => entry.role.trim().toLowerCase() === role,
    );
    if (
      matches.length !== 1 ||
      !/^[0-9a-f]{64}$/.test(matches[0].agentPubkey)
    ) {
      throw new Error(
        `Prepare could not resolve one installed managed identity for the selected ${role} role. Re-scan the role packs and try again.`,
      );
    }
    await input.start(matches[0].agentPubkey);
  }
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
    await run("start_roles", input.dependencies.startRoles);
    await run("provision_provider", input.dependencies.provisionProvider);
    await run("start_provider", input.dependencies.startProvider);
    await run("refresh_runtime", input.dependencies.refreshRuntime);
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
