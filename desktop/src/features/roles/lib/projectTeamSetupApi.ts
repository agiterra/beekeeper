import { invokeTauri } from "@/shared/api/tauri";
import type {
  ProjectTeamSetupAuthoringReservation,
  ProjectTeamSetupDraft,
  ProjectTeamSetupLaunch,
  ProjectTeamSetupSnapshot,
  ProjectTeamSetupValidation,
} from "./projectTeamSetup";

export type ProjectTeamSetupScope = {
  projectRef: string;
  expectedRelayUrl: string;
};

type AuthoringScope = ProjectTeamSetupScope & { setupId: string };

/** Read reserved IDs without creating or publishing any event. */
export function getProjectTeamSetupAuthoring(input: AuthoringScope) {
  return invokeTauri<ProjectTeamSetupAuthoringReservation | null>(
    "project_team_setup_get_authoring",
    input,
  );
}

/** Preserve the exact session/create IDs before launch, including on retries. */
export function reserveProjectTeamSetupAuthoring(
  input: AuthoringScope & { channelId: string },
) {
  return invokeTauri<ProjectTeamSetupAuthoringReservation>(
    "project_team_setup_reserve_authoring",
    input,
  );
}

/** Read the launch and its receipt; never retry its publication implicitly. */
export function getProjectTeamSetupLaunch(input: AuthoringScope) {
  return invokeTauri<ProjectTeamSetupLaunch | null>(
    "project_team_setup_get_launch",
    input,
  );
}

/** Explicitly start or retry the same durably signed authoring request. */
export function startProjectTeamSetupAuthoring(
  input: AuthoringScope & {
    providerPubkey: string;
    providerInstanceRef: string;
    runtime: string;
    model: string;
  },
) {
  return invokeTauri<ProjectTeamSetupLaunch>(
    "project_team_setup_start_authoring",
    input,
  );
}

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
