import type { ProjectInstalledRoles } from "@/features/roles/lib/projectInstalledRoles";
import type { ProjectTeamSetupActivation } from "@/features/roles/lib/projectTeamSetup";

/**
 * Seed for the project setup read commands, set with `page.addInitScript`
 * before `installMockBridge`. Every command here is read-only on the native
 * side, so the mock never records or advances anything either.
 */
export type MockProjectTeamSetupSeed = {
  /** `project_team_list_installed_roles`; defaults to none installed. */
  installedRoles?: ProjectInstalledRoles[];
  /** `project_team_setup_get_brief` text; defaults to a rendered stand-in. */
  brief?: string;
  /** `project_team_setup_get_activation` keyed by publication id. */
  activations?: Record<string, ProjectTeamSetupActivation>;
};

declare global {
  interface Window {
    __BUZZ_E2E_PROJECT_TEAM_SETUP__?: MockProjectTeamSetupSeed;
  }
}

type Args = Record<string, unknown> | null;

function text(args: Args, key: string): string {
  const value = args?.[key];
  return typeof value === "string" ? value : "";
}

/** Handles the setup reads; `undefined` means "not one of mine". */
export function handleMockProjectTeamSetupCommand(
  command: string,
  args: Args,
): { value: unknown } | undefined {
  const seed = window.__BUZZ_E2E_PROJECT_TEAM_SETUP__ ?? {};
  switch (command) {
    case "project_team_list_installed_roles": {
      const relay = text(args, "expectedRelayUrl");
      if (!relay) throw { code: "invalid_input", message: "Relay required." };
      return { value: seed.installedRoles ?? [] };
    }
    case "project_team_setup_get_brief":
      return {
        value: {
          text:
            seed.brief ??
            [
              "Build a useful baseline team for this project.",
              `Project: ${text(args, "projectRef")}`,
              `Setup: ${text(args, "setupId")}`,
            ].join("\n\n"),
        },
      };
    // A fresh project has no saved draft or publication. The Roles page reads
    // both on mount; answering "none" keeps a spec that never set up a
    // project from rendering a read failure. Specs that exercise setup wrap
    // `invoke` before the bridge, so their own answers still win.
    case "project_team_setup_get":
    case "project_team_setup_get_publication":
    case "project_team_setup_peek_publication":
      return { value: null };
    case "project_team_setup_get_activation": {
      const activation = seed.activations?.[text(args, "publicationId")];
      if (!activation)
        throw {
          code: "invalid_publication",
          message: "No mocked activation for this publication.",
        };
      return { value: activation };
    }
    default:
      return undefined;
  }
}
