/**
 * Whether the agents a project setup installed can actually be hired by the
 * project's lead on this computer.
 *
 * The hire host seats only agents whose local record carries this project's
 * association (`ManagedAgent.projectRef`, read through
 * `shared/lib/projectAgentAssociation`). An installation journal entry, a
 * matching role name or an installed pack is not that association, so setup
 * must never read as ready or complete while an installed role's agent lacks
 * it. This module is pure: the setup stage model and the roster card both
 * read it, so the header and the card cannot disagree.
 */
import type { ManagedAgent } from "@/shared/api/types";
import { agentProjectRelation } from "@/shared/lib/projectAgentAssociation";

/**
 * How one installed role's agent relates to the setup's project on this
 * computer.
 *
 * - `associated` — the local record belongs to exactly this project.
 * - `not-associated` — the local record belongs to no project.
 * - `other-project` — the local record belongs to a different project; it is
 *   never borrowed.
 * - `missing` — this computer has no record for the installed pubkey.
 * - `unknown` — the agents on this computer were not (or could not be) read.
 */
export type ProjectRosterAssociation =
  | "associated"
  | "not-associated"
  | "other-project"
  | "missing"
  | "unknown";

export type ProjectRosterEntry = {
  role: string;
  agentPubkey: string;
  /** The local record's current name; `null` when there is no record. */
  name: string | null;
  association: ProjectRosterAssociation;
  isLead: boolean;
};

export type ProjectRosterStatus =
  /** The agents on this computer have not been read yet. */
  | "checking"
  /** Reading the agents on this computer failed. */
  | "unreadable"
  /** The installation recorded no agents at all. */
  | "empty"
  /** Some installed role's agent is not this project's agent here. */
  | "blocked"
  /** Every installed role's agent is this project's agent here. */
  | "ready";

export type ProjectRosterReadiness = {
  status: ProjectRosterStatus;
  entries: ProjectRosterEntry[];
  /** Entries whose association is neither `associated` nor `unknown`. */
  blocked: ProjectRosterEntry[];
  /** The lead's current name, when its record was read. */
  leadName: string | null;
};

/**
 * `agents`: the managed agents on this computer; `undefined`/`null` while
 * unread, `"unreadable"` when the read failed.
 */
export type ProjectRosterReadinessInput = {
  projectRef: string;
  installedRoles: readonly { role: string; agentPubkey: string }[];
  agents: readonly ManagedAgent[] | null | undefined | "unreadable";
  leadPubkey: string | null;
};

function associationOf(
  agent: ManagedAgent | undefined,
  projectRef: string,
): ProjectRosterAssociation {
  if (!agent) return "missing";
  switch (agentProjectRelation(agent, projectRef)) {
    case "project":
      return "associated";
    case "other-project":
      return "other-project";
    case "unassociated":
      return "not-associated";
  }
}

export function projectRosterReadiness(
  input: ProjectRosterReadinessInput,
): ProjectRosterReadiness {
  const read = Array.isArray(input.agents)
    ? (input.agents as readonly ManagedAgent[])
    : null;
  const byKey = new Map(
    (read ?? []).map((agent) => [agent.pubkey.toLowerCase(), agent]),
  );
  const lead = input.leadPubkey?.toLowerCase() ?? null;
  const seen = new Set<string>();
  const entries: ProjectRosterEntry[] = [];
  for (const role of input.installedRoles) {
    const key = role.agentPubkey.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    const agent = byKey.get(key);
    entries.push({
      role: role.role,
      agentPubkey: role.agentPubkey,
      name: agent?.name ?? null,
      association: read ? associationOf(agent, input.projectRef) : "unknown",
      isLead: key === lead,
    });
  }
  const blocked = entries.filter(
    (entry) =>
      entry.association !== "associated" && entry.association !== "unknown",
  );
  const leadName = lead === null ? null : byKey.get(lead)?.name?.trim() || null;
  const status: ProjectRosterStatus =
    input.agents === "unreadable"
      ? "unreadable"
      : !read
        ? "checking"
        : entries.length === 0
          ? "empty"
          : blocked.length > 0
            ? "blocked"
            : "ready";
  return { status, entries, blocked, leadName };
}

/** "Bob", "Bob and Gordan", "Bob, Gordan and Ira". */
export function joinNames(names: readonly string[]): string {
  if (names.length <= 1) return names[0] ?? "";
  return `${names.slice(0, -1).join(", ")} and ${names.at(-1)}`;
}

/** What a sentence calls an entry: its name, else "the <role> agent". */
export function rosterEntryName(entry: ProjectRosterEntry): string {
  return entry.name?.trim() || `the ${entry.role} agent`;
}

function capitalize(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

const REMEDY_ASSOCIATE =
  "Retry installation (safe, keeps identities) or associate them on the project's Agents tab.";
const REMEDY_ASSOCIATE_ONE =
  "Retry installation (safe, keeps identities) or associate it on the project's Agents tab.";
const REMEDY_OTHER_PROJECT =
  "Reinstalling can't move another project's agent here; review this project's agents on its Agents tab.";
const REMEDY_REINSTALL =
  "Retry installation (safe, keeps identities), then check the project's Agents tab.";

/**
 * The one sentence for a blocked roster. It names who the lead can't hire,
 * says a different project's agent is not borrowed, and names the remedy.
 * `leadStarted` changes only the opening: a started lead still can't hire.
 */
export function projectRosterBlockedSentence(
  readiness: Pick<ProjectRosterReadiness, "blocked">,
  leadStarted: boolean,
): string {
  const workers = readiness.blocked.filter((entry) => !entry.isLead);
  const leadEntry = readiness.blocked.find((entry) => entry.isLead) ?? null;
  const parts: string[] = [];
  if (workers.length > 0) {
    const names = joinNames(workers.map(rosterEntryName));
    const one = workers.length === 1;
    parts.push(
      `${
        leadStarted
          ? `Lead started, but it can't hire ${names}`
          : `The lead can't hire ${names} yet`
      }: ${one ? "it isn't" : "they aren't"} associated with this project on this computer.`,
    );
    if (leadEntry)
      parts.push(
        `${capitalize(rosterEntryName(leadEntry))}, the project lead, isn't either.`,
      );
  } else if (leadEntry) {
    const name = rosterEntryName(leadEntry);
    parts.push(
      leadStarted
        ? `Lead started, but ${name} isn't associated with this project on this computer.`
        : `${capitalize(name)}, the project lead, isn't associated with this project on this computer.`,
    );
  }
  const otherProject = readiness.blocked.filter(
    (entry) => entry.association === "other-project",
  );
  if (otherProject.length > 0)
    parts.push(
      otherProject.length === 1
        ? `${capitalize(rosterEntryName(otherProject[0]))} belongs to another project and isn't borrowed.`
        : `${capitalize(joinNames(otherProject.map(rosterEntryName)))} belong to another project and aren't borrowed.`,
    );
  const missing = readiness.blocked.filter(
    (entry) => entry.association === "missing",
  );
  if (missing.length > 0)
    parts.push(
      missing.length === 1
        ? `${capitalize(rosterEntryName(missing[0]))} has no agent record on this computer.`
        : `${capitalize(joinNames(missing.map(rosterEntryName)))} have no agent records on this computer.`,
    );
  const associable = readiness.blocked.filter(
    (entry) => entry.association === "not-associated",
  );
  if (otherProject.length > 0) {
    // Reinstalling never moves another project's agent here, so the remedy
    // cannot be a retry; the Agents tab is where this project's roster is
    // reviewed.
    parts.push(REMEDY_OTHER_PROJECT);
    return parts.join(" ");
  }
  parts.push(
    associable.length === 0
      ? REMEDY_REINSTALL
      : associable.length < readiness.blocked.length
        ? `Retry installation (safe, keeps identities) or associate ${joinNames(associable.map(rosterEntryName))} on the project's Agents tab.`
        : associable.length === 1
          ? REMEDY_ASSOCIATE_ONE
          : REMEDY_ASSOCIATE,
  );
  return parts.join(" ");
}

/** The completion sentence, once every agent is this project's and the lead started. */
export function projectRosterCompleteSentence(
  leadName: string | null,
  projectName: string | null | undefined,
): string {
  const project = projectName?.trim() || "this project";
  if (!leadName)
    return `Setup complete. The project lead leads ${project}. Give the lead a task in its session; it lists the team with \`bee projects agents\` and hires only these agents.`;
  return `Setup complete. ${leadName} leads ${project}. Give ${leadName} a task in its session; it lists the team with \`bee projects agents\` and hires only these agents.`;
}

/** The short per-row label for an entry's association. */
export function projectRosterAssociationLabel(
  association: ProjectRosterAssociation,
): string {
  switch (association) {
    case "associated":
      return "Project agent";
    case "not-associated":
      return "Not associated with this project";
    case "other-project":
      return "Belongs to another project";
    case "missing":
      return "No agent record on this computer";
    case "unknown":
      return "Checking association…";
  }
}
