import type { ProjectInstalledRole } from "@/features/roles/lib/projectInstalledRoles";
import { agentMaySeatInProject } from "@/shared/lib/projectAgentAssociation";
import { truncatePubkey } from "@/shared/lib/pubkey";

/**
 * The fields grouping reads; the lead and bench candidates both carry them.
 *
 * `projectRef` is the agent's durable association (`ManagedAgent.projectRef`),
 * the one fact eligibility reads. A matching role name, an installed pack or a
 * past seat is never evidence (`shared/lib/projectAgentAssociation.ts`).
 */
type Candidate = {
  pubkey: string;
  name: string;
  role: string | null;
  projectRef?: string | null;
};

/** One labelled block of the "Who leads" picker or the bench. */
export type CodingSessionCandidateGroup<T extends Candidate> = {
  id: "project" | "unassociated";
  /** Null for the flat list a projectless session shows. */
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

/** The heading naming the project whose agents are listed. */
export function codingSessionProjectAgentsHeading(
  projectName: string | null,
): string {
  const name = projectName?.trim();
  return name ? `${name} agents` : "This project's agents";
}

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
 * Apply the project's installed role to each candidate it names — as the
 * **label** only.
 *
 * The setup journal records what the identity was installed *for in this
 * project*, which is the role its pack was staged under. It never decides who
 * may be picked: that is the association alone.
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
 * Split candidates into the ones this session may pick and the ones it may
 * not.
 *
 * - `eligible`: a role, and associated with exactly `projectRef` — or, for a
 *   projectless session (`projectRef === null`), associated with no project.
 * - `excluded`: a role, but not eligible. These are what the count sentence
 *   under a picker counts. An agent with no role is in neither list: it could
 *   not lead or be benched in any session, so it is not "excluded here".
 */
export function partitionCodingSessionCandidates<T extends Candidate>(input: {
  candidates: readonly T[];
  projectRef: string | null;
}): { eligible: T[]; excluded: T[] } {
  const eligible: T[] = [];
  const excluded: T[] = [];
  for (const candidate of input.candidates) {
    if (candidate.role === null) continue;
    if (agentMaySeatInProject(candidate, input.projectRef)) {
      eligible.push(candidate);
    } else {
      excluded.push(candidate);
    }
  }
  return { eligible, excluded };
}

/**
 * The picker's options: only the agents this session may pick.
 *
 * A project session lists its own agents under a heading naming the project;
 * a projectless session lists unassociated agents as one flat block. Another
 * project's agents, and a projectless session's associated agents, are never
 * options — they are counted by {@link codingSessionCandidateExclusionSentence}.
 */
export function groupCodingSessionCandidates<T extends Candidate>(input: {
  candidates: readonly T[];
  projectRef: string | null;
  projectName: string | null;
}): CodingSessionCandidateGroup<T>[] {
  const { eligible } = partitionCodingSessionCandidates(input);
  if (input.projectRef === null) {
    return [{ id: "unassociated", heading: null, candidates: eligible }];
  }
  return [
    {
      id: "project",
      heading: codingSessionProjectAgentsHeading(input.projectName),
      candidates: eligible,
    },
  ];
}

/** What a sentence under a picker is about. */
export type CodingSessionCandidateSurface = "lead" | "bench";

/**
 * The one sentence under the lead picker or the bench that says who is not
 * listed, and why — or null when nobody was left out.
 */
export function codingSessionCandidateExclusionSentence(input: {
  excludedCount: number;
  projectRef: string | null;
  projectName: string | null;
  surface: CodingSessionCandidateSurface;
}): string | null {
  const count = input.excludedCount;
  if (count <= 0) return null;
  const one = count === 1;
  const agents = one ? "1 agent" : `${count} agents`;
  const pronoun = one ? "it" : "they";
  const verb = input.surface === "lead" ? "lead" : "be benched";
  if (input.projectRef === null) {
    return `${agents} on this computer ${one ? "belongs" : "belong"} to a project, so ${pronoun} can't ${verb} in a session outside one.`;
  }
  const name = input.projectName?.trim();
  const what = name
    ? one
      ? `a ${name} agent`
      : `${name} agents`
    : one
      ? "an agent of this project"
      : "agents of this project";
  return `${agents} on this computer ${one ? "isn't" : "aren't"} ${what}, so ${pronoun} can't ${verb} here. Associate one on the project's Agents tab.`;
}

/**
 * What the lead picker says when it has nothing to offer — or null when it
 * has options.
 */
export function codingSessionLeadEmptySentence(input: {
  eligibleCount: number;
  projectRef: string | null;
  projectName: string | null;
}): string | null {
  if (input.eligibleCount > 0) return null;
  if (input.projectRef === null) {
    return "No agent on this computer outside a project carries a role, so nobody can lead a team in a session outside a project. Switch to Solo, or start the session in a project.";
  }
  const name = input.projectName?.trim() || "this project";
  return `No ${name} agent with a role is on this computer, so nobody can lead a team here. Finish the project's role setup, or associate an agent on the project's Agents tab — or switch to Solo.`;
}

/**
 * The lead to preselect: the one agent of this project whose role is `lead`,
 * or null when there is none or more than one — two leads is a choice for the
 * person, not for this function. A projectless session has no default.
 */
export function codingSessionProjectLeadDefault(input: {
  candidates: readonly Candidate[];
  projectRef: string | null;
}): string | null {
  if (input.projectRef === null) return null;
  const { eligible } = partitionCodingSessionCandidates(input);
  const leads = new Map<string, string>();
  for (const candidate of eligible) {
    if (candidate.role !== "lead") continue;
    const key = candidate.pubkey.toLowerCase();
    if (!leads.has(key)) leads.set(key, candidate.pubkey);
  }
  if (leads.size !== 1) return null;
  const [only] = leads.values();
  return only ?? null;
}

/**
 * The lead actor the form reads: an explicit pick — including an explicit
 * "nobody" — and otherwise the project's default. A late result can change
 * the default; it can never replace a pick.
 *
 * A pick that is not among `eligible` resolves to null (unset), never to the
 * default: a selection made before the channel's project was read, or before
 * the agent was associated elsewhere, is stale, and seating it would pull an
 * agent into a project it does not belong to.
 */
export function resolveCodingSessionLeadActor(input: {
  selection: { actor: string | null; explicit: boolean };
  defaultActor: string | null;
  eligible: readonly { pubkey: string }[];
}): string | null {
  const actor = input.selection.explicit
    ? input.selection.actor
    : input.defaultActor;
  if (actor === null) return null;
  const key = actor.toLowerCase();
  return input.eligible.some((entry) => entry.pubkey.toLowerCase() === key)
    ? actor
    : null;
}
