import {
  AGENT_NO_ROLE_PACK_LABEL,
  AGENT_NO_ROLE_PACK_REMEDY,
  AGENT_SHARED_HOME_LABEL,
  AGENT_SHARED_HOME_REMEDY,
} from "@/features/agents/ui/AgentHomeRoleBadges";
import { formatCoordinationAge } from "@/shared/coordination/sessionCoordinationFormat";

import type { ReportedRolePackRelation } from "../lib/rolePackSnapshots";
import type { RolePackProvenanceState } from "../lib/rolePackProvenance";
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
  "No roles resolve for this project, and no seat or agent names one.";

/** A project id the route named that this reader cannot read (§B). */
export const PROJECT_PACKS_MISSING = "This project is not readable here.";

/** The button that opens the pack installer, moved here from the Agents tab. */
export const INSTALL_ROLES_BUTTON_LABEL = "Install roles";

export const AGENTS_BY_PROJECT_TITLE = "Agents by project";

export const UNPLACED_TITLE = "Unplaced";

export const ROLE_SKILLS_TITLE = "Skills";

export const ROLE_AGENTS_TITLE = "Agents";

export const ROLE_SEATS_TITLE = "Seats";

export const ROLE_SKILLS_EMPTY = "no skills";

export const ROLE_AGENTS_EMPTY = "no agents carry this role";

export const ROLE_SEATS_EMPTY = "no open seats";

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

function joinWithAnd(items: readonly string[]): string {
  if (items.length <= 1) return items[0] ?? "";
  if (items.length === 2) return `${items[0]} and ${items[1]}`;
  return `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`;
}

/**
 * The header sentence (Fix 3): "<n> role packs for <project>, from the
 * <origin> rung at <location>, pinned to <sha8>." — one sentence naming the
 * project, the rung(s), the location and an 8-char sha. `shaTitle` carries
 * the full 40-hex sha for the tooltip; it is `null` whenever the sentence
 * itself is not naming one known sha (no packs, unknown sha, or shas that
 * differ by role — showing one of them in the tooltip would misattribute it).
 */
export function packsSourceSentence(
  projectName: string,
  packCount: number,
  source: PacksSourceSummary,
): { text: string; shaTitle: string | null } {
  if (packCount === 0) {
    return {
      text: `No role packs resolve for ${projectName}.`,
      shaTitle: null,
    };
  }
  const countWord = packCount === 1 ? "1 role pack" : `${packCount} role packs`;
  const rungWord = source.origins.length > 1 ? "rungs" : "rung";
  const location = source.location ?? "several locations";
  const prefix = `${countWord} for ${projectName}, from the ${joinWithAnd(
    source.origins,
  )} ${rungWord} at ${location}`;
  if (source.shasDiffer) {
    return {
      text: `${prefix}, pinned to different shas by role.`,
      shaTitle: null,
    };
  }
  if (source.sha === null) {
    return { text: `${prefix}, with no sha recorded.`, shaTitle: null };
  }
  const isFullSha = /^[0-9a-f]{40}$/i.test(source.sha);
  return {
    text: `${prefix}, pinned to ${shaText(source.sha)}.`,
    shaTitle: isFullSha ? source.sha : null,
  };
}

/**
 * A reported row's relation to this machine's packs checkout — the revision
 * comparison's own vocabulary, plus the two outcomes it never answers
 * (`different-source`, `incomplete`). Final wording from the design: nothing
 * here claims a machine is "current" without a matching relation, and
 * `earlier`/`later` always disclose the commit count rather than rounding it
 * away.
 */
export function revisionRelationText(
  relation: ReportedRolePackRelation,
  behind: number | null,
  ahead: number | null,
): string {
  switch (relation) {
    case "current":
      return "Same revision as this machine";
    case "earlier":
      return `Earlier revision · ${behind ?? "an unknown number"} behind this machine`;
    case "later":
      return `Newer than this machine's copy · ${ahead ?? "an unknown number"} ahead — this machine has not refreshed`;
    case "unrelated":
      return "Different history from this machine's copy";
    case "unknown-here":
      return "Revision unknown to this machine";
    case "different-source":
      return "Different pack source";
    case "shipped-differs":
      return "Shipped defaults from a different app version";
    case "incomplete":
      return "Version claim incomplete";
  }
}

/** `reported 5m ago`, or the honest "time not reported" when `ageSeconds` is `null`. */
export function reportedAgoText(ageSeconds: number | null): string {
  return ageSeconds === null
    ? "time not reported"
    : `reported ${formatCoordinationAge(ageSeconds)} ago`;
}

/** The one sentence a non-current, non-terminal reported row adds. */
export const ADOPTION_KEEPS_UNTIL_NEXT_GENERATION =
  "Keeps this revision until its next launch or resume.";

/**
 * The Revision-snapshots section's own subtitle (final copy, per review).
 * Deliberately never says a commissioned label proves which role
 * instructions ran — only that Beekeeper checked who sent the report.
 */
export const ROLE_PACK_SNAPSHOTS_SUBTITLE =
  "Versions found on this machine and pack revisions reported in this project’s channels. Beekeeper checks who sent each report. Confirming its source does not prove which role instructions were used.";

/**
 * The three provenance labels, final wording (per review). `commissioned`
 * never says "verified execution" or "adopted" — it names only what was
 * checked: the signer of this report is the provider a founder-signed
 * lifecycle chain named for this exact generation.
 */
const PROVENANCE_STATE_LABEL: Record<RolePackProvenanceState, string> = {
  commissioned: "Reported by the assigned provider",
  "proof-unavailable": "Unverified · proof unavailable",
  disputed: "Disputed",
};

export const PROVENANCE_LABEL_COMMISSIONED =
  PROVENANCE_STATE_LABEL.commissioned;
export const PROVENANCE_LABEL_PROOF_UNAVAILABLE =
  PROVENANCE_STATE_LABEL["proof-unavailable"];
export const PROVENANCE_LABEL_DISPUTED = PROVENANCE_STATE_LABEL.disputed;

/** The provenance chip's text: the state's label, then ` · <reason>` when present. */
export function provenanceLabelText(
  state: RolePackProvenanceState,
  reason: string | null,
): string {
  const label = PROVENANCE_STATE_LABEL[state];
  return reason ? `${label} · ${reason}` : label;
}

/** The reported section's one-line summary of the provenance fold's own notes. */
export function provenanceNotesSentence(notes: readonly string[]): string {
  return `Proof reads: ${notes.join(" ")}`;
}

/** The "Available here" heading's own commit fact, once git has answered. */
export function checkoutAnsweredSentence(
  currentSha: string,
  answeredAt: string,
): string {
  return `On ${shaText(currentSha)} · git answered at ${answeredAt}.`;
}

/** The revision comparison's disclosed failure, attributed. */
export function revisionComparisonUnavailableSentence(error: string): string {
  return `Revision comparison unavailable: ${error}`;
}
