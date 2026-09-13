import { useQuery } from "@tanstack/react-query";

import { listProjectInstalledRoles } from "./projectInstalledRoles";
import {
  projectTeamSetupBlocker,
  type ProjectTeamSetupDraft,
  type ProjectTeamSetupPublication,
} from "./projectTeamSetup";
import {
  getProjectTeamSetup,
  peekProjectTeamSetupPublication,
} from "./projectTeamSetupApi";

/**
 * A saved draft and what its host journals last recorded. These are records,
 * not live checks: nothing here re-observes the relay or the repository.
 */
export type ProjectTeamSetupSummary = {
  draft: ProjectTeamSetupDraft;
  /** `undefined` when the publication record could not be read. */
  publication: Pick<ProjectTeamSetupPublication, "status"> | null | undefined;
  /** Roles recorded as installed for this setup, when that was read. */
  installedRoleCount: number | null;
} | null;

/**
 * The only native commands the Roles page may invoke before setup is opened.
 * Each is strictly read-only: no reconciliation, lock or save.
 */
export const PROJECT_TEAM_SETUP_MOUNT_COMMANDS = [
  "project_team_setup_get",
  "project_team_setup_peek_publication",
  "project_team_list_installed_roles",
] as const;

export function projectTeamSetupSummaryQueryKey(
  projectRef: string,
  relayUrl: string,
) {
  return ["project-team-setup-summary", projectRef, relayUrl] as const;
}

/**
 * Read the saved draft and its last recorded publication and installation.
 *
 * Deliberately never calls `project_team_setup_get_publication`,
 * `…_get_publication_options`, `…_get_launch` or `…_get_activation`: those
 * re-observe remote state and some save what they observe, which a page
 * visit must never do.
 */
export async function loadProjectTeamSetupSummary(
  projectRef: string,
  relayUrl: string,
): Promise<ProjectTeamSetupSummary> {
  const scope = { projectRef, expectedRelayUrl: relayUrl };
  const draft = await getProjectTeamSetup(scope);
  if (!draft) return null;
  const publication = draft.latestSnapshotId
    ? await peekProjectTeamSetupPublication({
        ...scope,
        setupId: draft.setupId,
      }).catch(() => undefined)
    : null;
  let installedRoleCount: number | null = null;
  if (publication?.status === "adopted") {
    const installations = await listProjectInstalledRoles({
      expectedRelayUrl: relayUrl,
    }).catch(() => null);
    const entry = installations?.find(
      (item) =>
        item.setupId === draft.setupId &&
        item.projectRef.toLowerCase() === projectRef.toLowerCase(),
    );
    installedRoleCount = installations ? (entry?.roles.length ?? 0) : null;
  }
  return { draft, publication, installedRoleCount };
}

const RECORDED_PUBLICATION: Record<
  ProjectTeamSetupPublication["status"],
  string
> = {
  checking: "publishing started, not finished",
  candidate_prepared: "publishing started, not finished",
  push_unknown: "push not confirmed",
  pushed: "pushed, not yet adopted",
  source_unknown: "adoption not confirmed",
  adopted: "published and adopted",
  superseded: "replaced by a later published version",
  conflict: "publication conflicted",
  refused: "publication refused",
};

/** The Roles page's one line, phrased as a record rather than a live check. */
export function projectTeamSetupRecordedLine(
  summary: NonNullable<ProjectTeamSetupSummary>,
): string {
  const saved = "A project roles draft is saved on this computer.";
  if (!summary.draft.latestSnapshotId)
    return `${saved} Open setup to continue authoring and checking it.`;
  if (summary.publication === undefined)
    return `${saved} Its publication record couldn't be read; open setup to check.`;
  if (summary.publication === null)
    return `${saved} Last recorded: a checked version is saved, not yet published.`;
  const status = RECORDED_PUBLICATION[summary.publication.status];
  if (summary.publication.status === "adopted" && summary.installedRoleCount)
    return `${saved} Last recorded: ${status}, roles installed on this computer.`;
  return `${saved} Last recorded: ${status}.`;
}

/** The Roles page's view of a saved setup draft; disabled without a scope. */
export function useProjectTeamSetupSummaryQuery(
  projectRef: string,
  relayUrl: string,
  enabled = true,
) {
  return useQuery({
    queryKey: projectTeamSetupSummaryQueryKey(projectRef, relayUrl),
    // Same scope rule as the setup button: never read for an unsaved project
    // or without a community to bind the draft to.
    enabled:
      enabled &&
      projectTeamSetupBlocker({
        projectRef,
        relayUrl,
        intent: "setup",
        projectDirectory: "selected",
      }) === null,
    queryFn: () => loadProjectTeamSetupSummary(projectRef, relayUrl),
    staleTime: 15_000,
    retry: false,
  });
}
