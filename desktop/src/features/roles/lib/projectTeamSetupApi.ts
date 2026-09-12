import { invokeTauri } from "@/shared/api/tauri";
import type {
  ProjectTeamSetupDraft,
  ProjectTeamSetupSnapshot,
  ProjectTeamSetupValidation,
} from "./projectTeamSetup";

export type ProjectTeamSetupScope = {
  projectRef: string;
  expectedRelayUrl: string;
};

/** Read a durable local draft without creating one. */
export function getProjectTeamSetup(scope: ProjectTeamSetupScope) {
  return invokeTauri<ProjectTeamSetupDraft | null>(
    "project_team_setup_get",
    scope,
  );
}

/** Verify the repository root and prepare isolated role-pack drafts. */
export function prepareProjectTeamSetup(
  input: ProjectTeamSetupScope & { intent: string; projectDirectory: string },
) {
  return invokeTauri<ProjectTeamSetupDraft>(
    "project_team_setup_prepare",
    input,
  );
}

/** Check local pack structure without publishing or changing adoption state. */
export function validateProjectTeamSetup(
  input: ProjectTeamSetupScope & { setupId: string },
) {
  return invokeTauri<ProjectTeamSetupValidation>(
    "project_team_setup_validate",
    input,
  );
}

/** Save a checked copy, or reverify the exact previously saved version. */
export function snapshotProjectTeamSetup(
  input: ProjectTeamSetupScope & { setupId: string; snapshotId?: string },
) {
  return invokeTauri<ProjectTeamSetupSnapshot>(
    "project_team_setup_snapshot",
    input,
  );
}
