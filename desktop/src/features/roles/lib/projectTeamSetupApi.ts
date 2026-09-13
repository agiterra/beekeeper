import { invokeTauri } from "@/shared/api/tauri";
import type {
  ProjectTeamSetupAuthoringReservation,
  ProjectTeamSetupActivation,
  ProjectTeamSetupBrief,
  ProjectTeamSetupDraft,
  ProjectTeamSetupLaunch,
  ProjectTeamSetupPublication,
  ProjectTeamSetupPublicationDestination,
  ProjectTeamSetupPublicationOptions,
  ProjectTeamSetupPublicationOutput,
  ProjectTeamSetupSnapshot,
  ProjectTeamSetupSourceExpectation,
  ProjectTeamSetupValidation,
} from "./projectTeamSetup";

export type ProjectTeamSetupScope = {
  projectRef: string;
  expectedRelayUrl: string;
};

type AuthoringScope = ProjectTeamSetupScope & { setupId: string };

type PublicationScope = AuthoringScope & { publicationId: string };

/** Read reserved IDs without creating or publishing any event. */
export function getProjectTeamSetupAuthoring(input: AuthoringScope) {
  return invokeTauri<ProjectTeamSetupAuthoringReservation | null>(
    "project_team_setup_get_authoring",
    input,
  );
}

/** Read the exact native brief this setup writes and sends; never writes it. */
export function getProjectTeamSetupBrief(input: AuthoringScope) {
  return invokeTauri<ProjectTeamSetupBrief>(
    "project_team_setup_get_brief",
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

/** Read host-selected, provenance-checked choices before publication can start. */
export function getProjectTeamSetupPublicationOptions(input: AuthoringScope) {
  return invokeTauri<ProjectTeamSetupPublicationOptions>(
    "project_team_setup_get_publication_options",
    input,
  );
}

/**
 * Reserve or resume the same durable publication. All source and destination
 * values are previously resolved by the host, never inferred from draft files.
 */
export function startProjectTeamSetupPublication(
  input: AuthoringScope & {
    destination: ProjectTeamSetupPublicationDestination;
    sourceExpectation: ProjectTeamSetupSourceExpectation;
    output: ProjectTeamSetupPublicationOutput;
  },
) {
  return invokeTauri<ProjectTeamSetupPublication>(
    "project_team_setup_start_publication",
    input,
  );
}

/**
 * Reconcile and return the publication journal. Not a pure read: the host
 * re-observes the repository and relay and may save what it observed, so
 * call it only from an explicitly opened setup dialog. For display on page
 * load, use `peekProjectTeamSetupPublication`.
 */
export function getProjectTeamSetupPublication(input: AuthoringScope) {
  return invokeTauri<ProjectTeamSetupPublication | null>(
    "project_team_setup_get_publication",
    input,
  );
}

/**
 * The publication journal's last recorded status, strictly read-only: no
 * reconciliation, no lock, no save. Safe on page mount; describe the result
 * as last recorded, never as a live check.
 */
export function peekProjectTeamSetupPublication(input: AuthoringScope) {
  return invokeTauri<ProjectTeamSetupPublication | null>(
    "project_team_setup_peek_publication",
    input,
  );
}

/** Retry the host-recorded publication operation without changing its scope. */
export function continueProjectTeamSetupPublication(input: PublicationScope) {
  return invokeTauri<ProjectTeamSetupPublication>(
    "project_team_setup_continue_publication",
    input,
  );
}

/** Read observed local installation and lead handoff state without advancing it. */
export function getProjectTeamSetupActivation(input: PublicationScope) {
  return invokeTauri<ProjectTeamSetupActivation>(
    "project_team_setup_get_activation",
    input,
  );
}

/** Install only the role packs resolved from this adopted immutable source. */
export function installProjectTeamSetupActivation(input: PublicationScope) {
  return invokeTauri<ProjectTeamSetupActivation>(
    "project_team_setup_install_adopted_roles",
    input,
  );
}

/** Persist the one project session channel before a lead can be launched there. */
export function recordProjectTeamSetupLeadChannel(
  input: PublicationScope & { channelId: string },
) {
  return invokeTauri<ProjectTeamSetupActivation>(
    "project_team_setup_record_lead_channel",
    input,
  );
}

/** Start or recover the one durably reserved project lead for this channel. */
export function startProjectTeamSetupLead(
  input: PublicationScope & { channelId: string },
) {
  return invokeTauri<ProjectTeamSetupActivation>(
    "project_team_setup_start_lead",
    input,
  );
}

/** Reserve and recover this publication's one project session channel. */
export function ensureProjectTeamSetupLeadChannel(input: PublicationScope) {
  return invokeTauri<ProjectTeamSetupActivation>(
    "project_team_setup_ensure_lead_channel",
    input,
  );
}
