import { invokeTauri } from "@/shared/api/tauri";

/**
 * Removing what a project created on *this computer*: its managed agent
 * identities, their keys, the `Project team <coordinate>` team, and the
 * `crew-role:` definitions the installer wrote.
 *
 * Its own module rather than `tauri.ts`, which sits above the repository's
 * 1000-line ceiling and is held flat by the ratchet.
 *
 * Two calls, deliberately: the plan is read-only and feeds the confirmation,
 * and the run re-enumerates under the host's store lock and refuses if the
 * set has moved since the plan was shown.
 */

export type TeardownAgent = { pubkey: string; name: string };

export type TeardownDefinition = {
  id: string;
  displayName: string;
  dTag: string;
};

export type ProjectAgentTeardownPlan = {
  projectRef: string;
  team: { id: string; name: string } | null;
  agents: TeardownAgent[];
  definitions: TeardownDefinition[];
  /**
   * Agents deployed to a remote provider. Their presence refuses the whole
   * teardown — deleting the local record would orphan a live deployment.
   */
  remoteDeployed: TeardownAgent[];
  /**
   * Sessions the provider still has open for this project. Disclosed, never
   * a refusal: nothing here can retire them, so the dialog names them rather
   * than implying they are handled.
   */
  liveSessions: string[];
  /** Paths left on disk on purpose, each with its reason. */
  retained: { path: string; reason: string }[];
};

export type ProjectAgentTeardownReceipt = {
  agentsDeleted: string[];
  definitionsRemoved: string[];
  teamDeleted: string | null;
  backups: string[];
  skipped: string[];
  /** True only when nothing was skipped. */
  complete: boolean;
};

/** Enumerate what a teardown would remove. Changes nothing. */
export function planProjectAgentTeardown(
  projectRef: string,
): Promise<ProjectAgentTeardownPlan> {
  return invokeTauri<ProjectAgentTeardownPlan>("plan_project_agent_teardown", {
    projectRef,
  });
}

/**
 * Remove them. `expectAgents` is the pubkey list the plan returned; the host
 * refuses if its own enumeration no longer matches.
 */
export function runProjectAgentTeardown(
  projectRef: string,
  expectAgents: readonly string[],
): Promise<ProjectAgentTeardownReceipt> {
  return invokeTauri<ProjectAgentTeardownReceipt>(
    "run_project_agent_teardown",
    {
      projectRef,
      expectAgents: [...expectAgents],
    },
  );
}
