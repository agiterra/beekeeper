import {
  AGENT_NO_ROLE_PACK_LABEL,
  AGENT_NO_ROLE_PACK_REMEDY,
  AGENT_SHARED_HOME_LABEL,
  AGENT_SHARED_HOME_REMEDY,
} from "@/features/agents/ui/AgentHomeRoleBadges";

import type { PacksSourceSummary } from "../lib/rolesViewModel";

/**
 * Every sentence the Roles tab shows, as values a test can assert.
 *
 * Nothing here softens a fact: a missing sha says "sha unknown", a role the
 * ladder does not know says so, and an agent without its pack carries the
 * same words the Agents tab already uses for it.
 */

export const ROLES_TITLE = "Roles";

export const ROLES_SUBTITLE =
  "What each role has, who carries it, and where every agent is seated. Packs are what this computer would stage for the chosen project; seat status is what each session last reported.";

export const ROLES_PROJECT_PICKER_LABEL = "Project";

export const ROLES_PROJECT_PICKER_ARIA = "Project whose role packs to show";

export const ROLES_NO_PROJECT = "No project to read packs for.";

export const ROLES_LOADING = "Reading role packs…";

export const ROLES_SECTION_TITLE = "Roles";

export const ROLES_EMPTY =
  "No roles: the ladder for this project produced no packs, and no seat or agent on this computer names one.";

export const AGENTS_BY_PROJECT_TITLE = "Agents by project";

export const UNPLACED_TITLE = "Unplaced";

export const ROLE_SKILLS_TITLE = "Skills";

export const ROLE_AGENTS_TITLE = "Agents";

export const ROLE_SEATS_TITLE = "Live seats";

export const ROLE_SKILLS_EMPTY = "no skills";

export const ROLE_AGENTS_EMPTY = "no agents carry this role";

export const ROLE_SEATS_EMPTY = "no seats running";

export const PROJECT_SEATS_EMPTY = "no agents seated";

export const SHA_UNKNOWN = "sha unknown";

export const SHARED_SKILL_MARK = "(shared)";

/** A role row the ladder produced no pack for. */
export const ROLE_NO_PACK =
  "No pack for this role on this computer's ladder — listed because a seat or an agent names it.";

/** A seat's agent column when the session has no agent (a person's session). */
export const SEAT_NO_AGENT = "no agent seated";

/** A seat's agent column when the agent is not managed on this computer. */
export const SEAT_UNMANAGED_AGENT = "unmanaged agent";

export const SEAT_NO_ROLE = "no role";

export const SEAT_NO_PROJECT = "unplaced";

export const AGE_UNKNOWN = "age unknown";

/** The tooltip on a chip whose agent has a home role and no pack here — the Agents tab's own words. */
export const AGENT_CHIP_NO_PACK_TITLE = `${AGENT_NO_ROLE_PACK_LABEL} — ${AGENT_NO_ROLE_PACK_REMEDY}`;

/** The tooltip on a chip whose pack is here and refused by a shared home — the Agents tab's own words. */
export const AGENT_CHIP_SHARED_HOME_TITLE = `${AGENT_SHARED_HOME_LABEL} — ${AGENT_SHARED_HOME_REMEDY}`;

/** The backend's sentence, attributed. */
export function rolesErrorSentence(message: string): string {
  return `Role packs could not be read: ${message}`;
}

/** Compact age: "just now", "5m", "2h", "3d"; `null` is disclosed, not zeroed. */
export function formatAge(seconds: number | null): string {
  if (seconds === null) return AGE_UNKNOWN;
  if (seconds < 60) return "just now";
  if (seconds < 3_600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86_400) return `${Math.floor(seconds / 3_600)}h`;
  return `${Math.floor(seconds / 86_400)}d`;
}

/** `running (5m)` — the catalog's status word verbatim, then its age. */
export function seatStatusText(
  status: string,
  ageSeconds: number | null,
): string {
  return `${status} (${formatAge(ageSeconds)})`;
}

/**
 * The sha as the seat pack line already words it — a 40-hex commit shortened
 * to 8 characters (`codingSessionSeatPackLine`), anything else (a shipped
 * pack's version string) verbatim — or the words for not having one.
 */
export function shaText(sha: string | null): string {
  if (sha === null) return SHA_UNKNOWN;
  return /^[0-9a-f]{40}$/i.test(sha) ? sha.slice(0, 8) : sha;
}

/** `3 agents` / `1 agent` / `0 agents`. */
export function projectSeatCount(count: number): string {
  return `${count} ${count === 1 ? "agent" : "agents"}`;
}

/**
 * The header sentence: "Packs for <project>: <origin> · <repo or path> · <sha>".
 *
 * A project whose roles come from more than one rung says "mixed origins"
 * and lists them; roles from several places say so; roles with different
 * shas say so rather than showing one of them.
 */
export function packsSourceSentence(
  projectName: string,
  packCount: number,
  source: PacksSourceSummary,
): string {
  if (packCount === 0) return `Packs for ${projectName}: none found`;
  const origin =
    source.origins.length === 1
      ? source.origins[0]
      : `mixed origins (${source.origins.join(", ")})`;
  const location = source.location ?? "several locations";
  const sha =
    source.sha ?? (source.shasDiffer ? "shas differ by role" : SHA_UNKNOWN);
  return `Packs for ${projectName}: ${origin} · ${location} · ${sha}`;
}
