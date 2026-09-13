import type { ProjectInstalledRole } from "@/features/roles/lib/projectInstalledRoles";
import { truncatePubkey } from "@/shared/lib/pubkey";

/** The fields grouping reads; the lead and bench candidates both carry them. */
type Candidate = { pubkey: string; name: string; role: string | null };

/** One labelled block of the "Who leads" picker or the bench. */
export type CodingSessionCandidateGroup<T extends Candidate> = {
  id: "project" | "other";
  /** Null only for the ungrouped list shown when the project installed none. */
  heading: string | null;
  candidates: T[];
};

/** `name · role · 1c47d440…5017` — enough of the key to tell same-named agents apart. */
export function codingSessionCandidateOptionLabel(
  candidate: Candidate,
): string {
  return [candidate.name, candidate.role, truncatePubkey(candidate.pubkey)]
    .filter((part): part is string => Boolean(part))
    .join(" · ");
}

/** The heading naming the project whose installed roles come first. */
export function codingSessionProjectRolesHeading(
  projectName: string | null,
): string {
  const name = projectName?.trim();
  return name ? `${name} project roles` : "This project's roles";
}

export const CODING_SESSION_OTHER_AGENTS_HEADING =
  "Other agents on this computer";

/**
 * The installed roles of this project that name an identity on this computer,
 * one per identity. A journal may record the same identity twice (a retried
 * install); the first entry wins.
 */
function installedByPubkey(
  installedRoles: readonly ProjectInstalledRole[],
): Map<string, ProjectInstalledRole> {
  const byPubkey = new Map<string, ProjectInstalledRole>();
  for (const entry of installedRoles) {
    const key = entry.agentPubkey.toLowerCase();
    if (!byPubkey.has(key)) byPubkey.set(key, entry);
  }
  return byPubkey;
}

/**
 * Apply the project's installed role to each candidate it names.
 *
 * The setup journal records what the identity was installed *for in this
 * project*, which is a stronger fact than a team record or its home role —
 * and it is the role the project's pack was staged under.
 */
export function applyCodingSessionInstalledRoles<T extends Candidate>(
  candidates: readonly T[],
  installedRoles: readonly ProjectInstalledRole[],
): T[] {
  const installed = installedByPubkey(installedRoles);
  return candidates.map((candidate) => {
    const entry = installed.get(candidate.pubkey.toLowerCase());
    return entry ? { ...candidate, role: entry.role } : candidate;
  });
}

/**
 * Split candidates into the project's installed roles, first, and every other
 * agent on this computer.
 *
 * With no project, or a project that installed nobody on this computer, the
 * list is returned as one ungrouped block — exactly the picker it was.
 */
export function groupCodingSessionCandidates<T extends Candidate>(input: {
  candidates: readonly T[];
  installedRoles: readonly ProjectInstalledRole[];
  projectRef: string | null;
  projectName: string | null;
}): CodingSessionCandidateGroup<T>[] {
  const installed = installedByPubkey(input.installedRoles);
  const project: T[] = [];
  const other: T[] = [];
  for (const candidate of input.candidates) {
    if (input.projectRef && installed.has(candidate.pubkey.toLowerCase())) {
      project.push(candidate);
    } else {
      other.push(candidate);
    }
  }
  if (project.length === 0) {
    return [{ id: "other", heading: null, candidates: other }];
  }
  const groups: CodingSessionCandidateGroup<T>[] = [
    {
      id: "project",
      heading: codingSessionProjectRolesHeading(input.projectName),
      candidates: project,
    },
  ];
  if (other.length > 0) {
    groups.push({
      id: "other",
      heading: CODING_SESSION_OTHER_AGENTS_HEADING,
      candidates: other,
    });
  }
  return groups;
}

/**
 * The lead to preselect: the one identity this project installed as `lead`
 * that can be seated here, or null when there is none or more than one — two
 * installed leads is a choice for the person, not for this function.
 */
export function codingSessionInstalledLeadDefault(input: {
  candidates: readonly Candidate[];
  installedRoles: readonly ProjectInstalledRole[];
}): string | null {
  const seatable = new Set(
    input.candidates
      .filter((candidate) => candidate.role !== null)
      .map((candidate) => candidate.pubkey.toLowerCase()),
  );
  const leads = new Set(
    input.installedRoles
      .filter((entry) => entry.role === "lead")
      .map((entry) => entry.agentPubkey.toLowerCase()),
  );
  if (leads.size !== 1) return null;
  const [only] = leads;
  if (!only || !seatable.has(only)) return null;
  return (
    input.candidates.find(
      (candidate) => candidate.pubkey.toLowerCase() === only,
    )?.pubkey ?? null
  );
}

/**
 * The lead actor the form reads: an explicit pick always — including an
 * explicit "nobody" — and otherwise the installed default. A late installed
 * roles result can change the default; it can never replace a pick.
 */
export function resolveCodingSessionLeadActor(input: {
  selection: { actor: string | null; explicit: boolean };
  installedDefault: string | null;
}): string | null {
  return input.selection.explicit
    ? input.selection.actor
    : input.installedDefault;
}
