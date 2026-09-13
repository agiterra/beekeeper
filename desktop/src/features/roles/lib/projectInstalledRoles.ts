import { useQuery } from "@tanstack/react-query";

import { invokeTauri } from "@/shared/api/tauri";

import type { PackRef } from "@/features/coding-sessions/lib/codingSessionPackRef";

/**
 * Which agent identities this computer installed for each project, read from
 * the project setup journals.
 *
 * This is the only durable link between an installed agent and a project:
 * a managed agent record carries no project field, and "has held a seat in
 * the project" (the directory's older signal) is a different fact that an
 * agent installed a minute ago cannot have yet. Installation is not
 * participation, and neither is permission — this list grants nothing.
 *
 * Read-only on the native side: listing never creates, repairs or publishes.
 */
export type ProjectInstalledRole = {
  role: string;
  agentPubkey: string;
  packRef: PackRef;
};

/** One project's installation, as its setup journal recorded it. */
export type ProjectInstalledRoles = {
  /** `30621:<owner>:<d>` — the NIP-MP project address. */
  projectRef: string;
  setupId: string;
  publicationId: string;
  teamId: string;
  /** The adopted immutable source the roles were installed from. */
  source: { repoRef: string; sha: string; packPath: string } | null;
  /** The session channel the setup reserved for the lead, when it has one. */
  leadChannelId: string | null;
  roles: ProjectInstalledRole[];
};

/** List installations for the active owner on this relay. */
export function listProjectInstalledRoles(input: { expectedRelayUrl: string }) {
  return invokeTauri<ProjectInstalledRoles[]>(
    "project_team_list_installed_roles",
    input,
  );
}

export function projectInstalledRolesQueryKey(expectedRelayUrl: string | null) {
  return ["project-installed-roles", expectedRelayUrl] as const;
}

/**
 * Installed roles for every project, or `[]` while unknown.
 *
 * `null` relay URL disables the query: a list read against the wrong relay
 * would attribute another community's installations to this one.
 */
export function useProjectInstalledRolesQuery(expectedRelayUrl: string | null) {
  return useQuery({
    queryKey: projectInstalledRolesQueryKey(expectedRelayUrl),
    enabled: expectedRelayUrl !== null,
    queryFn: () =>
      listProjectInstalledRoles({ expectedRelayUrl: expectedRelayUrl ?? "" }),
    staleTime: 15_000,
  });
}

/** The installed roles recorded for one project address, if any. */
export function installedRolesForProject(
  installations: readonly ProjectInstalledRoles[] | undefined,
  projectRef: string | null,
): ProjectInstalledRole[] {
  if (!installations || !projectRef) return [];
  const wanted = projectRef.toLowerCase();
  return installations
    .filter((entry) => entry.projectRef.toLowerCase() === wanted)
    .flatMap((entry) => entry.roles);
}

/** Every project address an agent was installed for. */
export function installedProjectRefsByAgent(
  installations: readonly ProjectInstalledRoles[] | undefined,
): Map<string, { projectRef: string; role: string }[]> {
  const byAgent = new Map<string, { projectRef: string; role: string }[]>();
  for (const entry of installations ?? []) {
    for (const role of entry.roles) {
      const key = role.agentPubkey.toLowerCase();
      const list = byAgent.get(key) ?? [];
      list.push({ projectRef: entry.projectRef, role: role.role });
      byAgent.set(key, list);
    }
  }
  return byAgent;
}
