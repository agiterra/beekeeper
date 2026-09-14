import {
  AGENT_NO_ROLE_PACK_LABEL,
  AGENT_NO_ROLE_PACK_REMEDY,
  AGENT_SHARED_HOME_LABEL,
  AGENT_SHARED_HOME_REMEDY,
} from "@/features/agents/ui/AgentHomeRoleBadges";
import type { RolePackOrigin, RolePackRef } from "@/shared/api/types";
import { formatCoordinationAge } from "@/shared/coordination/sessionCoordinationFormat";

import type { ReportedRolePackRelation } from "../lib/rolePackSnapshots";
import type { RolePackProvenanceState } from "../lib/rolePackProvenance";
import type { RoleReportSummary } from "../lib/roleVersionSummary";
import type { PacksSourceSummary, RoleAgentChip } from "../lib/rolesViewModel";

/**
 * Every sentence the Roles tab shows, as values a test can assert.
 *
 * Nothing here softens a fact: a missing sha says "version unknown", a role this
 * computer has no instructions for says so, and an agent without its pack
 * carries the same words the Agents tab already uses for it.
 *
 * Two vocabularies live here. The *primary* copy — the header, the source
 * line and the role cards — is written for someone who has never read a NIP:
 * no "seat", "pack", "rung", "coordinate", "genesis", "commissioned" or
 * "provider". The *diagnostic* copy, used only inside Technical details,
 * keeps the protocol's own long labels verbatim so an operator reading them
 * gets the exact claim; the card versions of the same facts are the short
 * forms below, which carry the long label in a `title`.
 */

export const ROLES_TITLE = "Roles";

/** The one line under the header: what this page answers. */
export const ROLES_SUBTITLE =
  "What each role is for, its project participants, and the versions available here or reported by agents.";

/**
 * The longer explanation the header used to carry. It is still true and still
 * the first thing an unfamiliar reader needs, so it is kept verbatim — moved
 * into Technical details, where a reader who wants it can open it, instead of
 * standing between the header and the roles every time.
 */
export const ROLES_EXPLANATION =
  "Each role is a set of instructions an agent follows in this project. Here you can see what each role is for, which agents can take it, and which version of its instructions is available on this computer and reported by running agents.";

/** The header control that re-reads every source this page already reads. */
export const ROLES_RECHECK_LABEL = "Check again";
export const ROLES_RECHECK_BUSY_LABEL = "Checking…";

export const ROLES_PROJECT_PICKER_LABEL = "Project";

export const ROLES_PROJECT_PICKER_ARIA = "Project whose role packs to show";

export const ROLES_NO_PROJECT = "No project to read packs for.";

export const ROLES_LOADING = "Reading roles…";

export const ROLES_EMPTY =
  "No roles are available for this project, and no agent or open session names one.";

/** A project id the route named that this reader cannot read (§B). */
export const PROJECT_PACKS_MISSING = "This project is not readable here.";

/** The button that opens the pack installer, moved here from the Agents tab. */
export const INSTALL_ROLES_BUTTON_LABEL = "Install roles";

/** Where the sessions list went: the Agents tab says who holds each role, and where. */
export const ROLES_AGENTS_POINTER =
  "Which agents hold these roles, in which sessions, and what they were assigned:";

export const ROLES_AGENTS_POINTER_LINK = "Agents";

export const ROLE_SKILLS_TITLE = "Skills";

/** The collapsed skills disclosure's summary: `Skills (3)`. */
export function roleSkillsSummary(count: number): string {
  return `${ROLE_SKILLS_TITLE} (${count})`;
}

/** The card's foot disclosure, which holds the long description and the skills. */
export const ROLE_ABOUT_TITLE = "About this role";

/** `About this role · Skills (3)`. */
export function roleAboutSummary(skillCount: number): string {
  return `${ROLE_ABOUT_TITLE} · ${roleSkillsSummary(skillCount)}`;
}

/** The collapsed reported-version disclosure's summary: `Versions (2)`. */
export function roleVersionsSummary(count: number): string {
  return `Versions (${count})`;
}

export const ROLE_AGENTS_TITLE = "Agents";

export const ROLE_SEATS_TITLE = "Sessions";

export const ROLE_SKILLS_EMPTY = "No skills listed.";

export const ROLE_AGENTS_EMPTY = "No agents yet.";

export const ROLE_SEATS_EMPTY = "No open sessions.";

/**
 * A role no agent holds, no session runs and no report names. One line
 * instead of three separate absences, which said the same thing three times.
 */
export const ROLE_QUIET =
  "No agents, sessions or reports observed for this role.";

/**
 * The one scope line on the page, under the agent count. Two different kinds
 * of agent are counted there and they are not equally known: one is a record
 * on this computer, the other was merely *seen* in this project. The line
 * names both rather than presenting their sum as one roster, and the tooltip
 * says plainly that the seen half is an observation, not a complete list.
 */
export function rolesScopeText(local: number, shared: number): string {
  const here = `${local} on this computer`;
  return shared === 0 ? here : `${here} · ${shared} shared`;
}

export const ROLES_SCOPE_TITLE =
  "Local agents are set up on this computer. Shared agents were seen in this project's sessions or channels; this is an observed list, not a complete roster.";

/** The badge on an agent this computer does not manage. */
export const AGENT_SHARED_BADGE = "shared";

export const AGENT_SHARED_TITLE =
  "Seen in this project's sessions or channels; not set up on this computer. Its status, instructions and model are not known here.";

/**
 * The pack disclosure for a shared agent. Not "no pack" and not "has one":
 * this computer resolved instructions for itself, and what another machine
 * staged is not something it was told.
 */
export const AGENT_CHIP_PACK_UNKNOWN_TITLE =
  "Whether this agent has this role's instructions on its own machine is not known here.";

export const SHA_UNKNOWN = "version unknown";

export const SHARED_SKILL_MARK = "(shared)";

/** A role this computer resolved no instructions for. */
export const ROLE_NO_PACK =
  "No instructions for this role are on this computer; it is listed because an agent or an open session names it.";

/** A session row's agent column when the session has no agent (a person's session). */
export const SEAT_NO_AGENT = "no agent";

/** A session row's agent column when the agent is not managed on this computer. */
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
  return `Role instructions could not be read: ${message}`;
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

function isCommitSha(sha: string | null): sha is string {
  return sha !== null && /^[0-9a-f]{40}$/i.test(sha);
}

/** What the source line and the card's availability line call each rung. */
const ORIGIN_PHRASE: Record<RolePackOrigin, string> = {
  project: "from the project's repository",
  checkout: "from this computer only",
  installed: "from this computer only",
  shipped: "from this app's built-in defaults",
};

/** No origin at all — never guessed from a directory name. */
const ORIGIN_PHRASE_UNRECORDED = "from a source this computer did not record";

/** Extra facts the source line names that `PacksSourceSummary` does not carry. */
export type PacksSourceDetail = {
  /** Seconds since the local resolution that produced this answer. */
  checkedAgeSeconds: number | null;
  /** The `version` every built-in role agrees on, or `null`. */
  shippedVersion: string | null;
  /**
   * A project roles setup draft is saved on this computer and not finished.
   * Only mentioned while the roles are still the built-in defaults.
   */
  draftInProgress?: boolean;
};

/** Appended to the built-in source line while a setup draft is unfinished. */
export const PACKS_SOURCE_DRAFT_IN_PROGRESS =
  "A project roles draft is in progress on this computer.";

const NO_SOURCE_DETAIL: PacksSourceDetail = {
  checkedAgeSeconds: null,
  shippedVersion: null,
};

/** Roles resolved from more than one rung — the page cannot name one place. */
export const PACKS_SOURCE_MIXED =
  "Roles come from more than one place; see Technical details.";

/**
 * The header's source line, in the words of someone who has never staged a
 * pack: where the instructions on this computer came from, and (for the
 * project's own repository) which commit and how long ago it was checked.
 *
 * It never averages. Mixed rungs say so and point at Technical details;
 * roles pinned to different commits say that instead of picking one;
 * `shaTitle` carries the full 40-hex commit only when one commit is named.
 */
export function packsSourceSentence(
  projectName: string,
  packCount: number,
  source: PacksSourceSummary,
  detail: PacksSourceDetail = NO_SOURCE_DETAIL,
): { text: string; shaTitle: string | null } {
  if (packCount === 0) {
    return {
      text: `No role instructions are available for ${projectName}.`,
      shaTitle: null,
    };
  }
  if (source.origins.length > 1) {
    return { text: PACKS_SOURCE_MIXED, shaTitle: null };
  }
  const origin = source.origins[0] ?? null;
  if (origin === "shipped") {
    const version = detail.shippedVersion ?? source.sha;
    const text =
      version === null
        ? "Instructions come from this app's built-in defaults."
        : `Instructions come from this app's built-in defaults (v${version}).`;
    return {
      text: detail.draftInProgress
        ? `${text} ${PACKS_SOURCE_DRAFT_IN_PROGRESS}`
        : text,
      shaTitle: null,
    };
  }
  if (origin === "checkout" || origin === "installed") {
    return {
      text: "Instructions come from this computer only (not shared with the project).",
      shaTitle: null,
    };
  }
  if (origin === null) {
    return {
      text: `Instructions come ${ORIGIN_PHRASE_UNRECORDED}.`,
      shaTitle: null,
    };
  }
  const checked =
    detail.checkedAgeSeconds === null
      ? " · last check time not recorded"
      : detail.checkedAgeSeconds < 60
        ? " · checked just now"
        : ` · checked ${formatAge(detail.checkedAgeSeconds)} ago`;
  if (source.shasDiffer) {
    return {
      text: `Instructions come from the project's repository, at a different commit for each role — see Technical details.${checked}`,
      shaTitle: null,
    };
  }
  if (source.sha === null) {
    return {
      text: `Instructions come from the project's repository, with no commit recorded.${checked}`,
      shaTitle: null,
    };
  }
  return {
    text: `Instructions come from the project's repository at ${shaText(source.sha)}${checked}`,
    shaTitle: isCommitSha(source.sha) ? source.sha : null,
  };
}

export const ROLE_AVAILABLE_PREFIX = "Available here: ";
export const ROLE_UNAVAILABLE_PREFIX = "Not available here — ";
export const ROLE_VERSION_UNRECORDED = "version not recorded";

export type RoleAvailability = {
  availability: "available" | "unavailable";
  text: string;
  /** The full 40-hex commit for the tooltip, or `null` when none is named. */
  title: string | null;
};

/**
 * The card's one line about this computer: which version of the role's
 * instructions is here and where it came from, or the reason there is none.
 *
 * Only the card's own values are used. A role with no version string and no
 * commit says "version not recorded" rather than borrowing the header's sha,
 * and a refused role carries the backend's own refusal sentence instead of a
 * version it would not stage.
 */
export type RoleVersionInput = {
  hasPack: boolean;
  version: string | null;
  origin: RolePackOrigin | null;
  packRef: RolePackRef | null;
  refusal: string | null;
};

/**
 * The two things this computer can name about a role's version: the version
 * string it recorded and the commit it pinned. Either can be absent; neither
 * is ever filled in from the other, and a shipped pack whose only version is
 * its `packRef.sha` is named from that sha rather than from nothing.
 */
function roleVersionNames(input: RoleVersionInput): {
  version: string | null;
  commit: string | null;
} {
  const sha = input.packRef?.sha ?? null;
  return {
    version:
      input.version !== null
        ? `v${input.version}`
        : input.origin === "shipped" && sha !== null
          ? `v${sha}`
          : null,
    commit: isCommitSha(sha) ? shaText(sha) : null,
  };
}

export function roleAvailabilitySentence(
  input: RoleVersionInput,
): RoleAvailability {
  if (!input.hasPack || input.refusal !== null) {
    return {
      availability: "unavailable",
      text: `${ROLE_UNAVAILABLE_PREFIX}${input.refusal ?? ROLE_NO_PACK}`,
      title: null,
    };
  }
  const sha = input.packRef?.sha ?? null;
  const { version, commit } = roleVersionNames(input);
  const named = [version, commit === null ? null : `(${commit})`].filter(
    (part): part is string => part !== null,
  );
  const originText =
    input.origin === null
      ? ORIGIN_PHRASE_UNRECORDED
      : ORIGIN_PHRASE[input.origin];
  return {
    availability: "available",
    text: `${ROLE_AVAILABLE_PREFIX}${named.length > 0 ? named.join(" ") : ROLE_VERSION_UNRECORDED} ${originText}`,
    title: isCommitSha(sha) ? sha : null,
  };
}

/** The card chip's words for a role with no instructions here. */
export const ROLE_NOT_AVAILABLE_CHIP = "Not available here";

/** The disclosure that carries the whole unavailable sentence on the card. */
export const ROLE_UNAVAILABLE_DETAILS_SUMMARY = "Why not available";

export type RoleVersionChip = {
  availability: "available" | "unavailable";
  /** The chip's own short text: `v1.3.0 · 9f2e1d0c`, or the unavailable words. */
  text: string;
  /** The whole sentence, for the tooltip and the unavailable disclosure. */
  sentence: string;
  /** The chip's tooltip. */
  title: string;
};

/**
 * The card's version chip: the shortest true form on the face, the whole
 * sentence in the tooltip. Nothing is dropped on the way in — a role with no
 * version and no commit says "version not recorded" rather than showing an
 * empty chip, an unavailable role says so on the face and carries its reason
 * in the tooltip, and the full 40-hex commit is appended to the tooltip
 * whenever one is named.
 */
export function roleVersionChip(input: RoleVersionInput): RoleVersionChip {
  const sentence = roleAvailabilitySentence(input);
  if (sentence.availability === "unavailable") {
    return {
      availability: "unavailable",
      text: ROLE_NOT_AVAILABLE_CHIP,
      sentence: sentence.text,
      title: input.refusal ?? ROLE_NO_PACK,
    };
  }
  const { version, commit } = roleVersionNames(input);
  const named = [version, commit].filter(
    (part): part is string => part !== null,
  );
  return {
    availability: "available",
    text: named.length > 0 ? named.join(" · ") : ROLE_VERSION_UNRECORDED,
    sentence: sentence.text,
    title:
      sentence.title === null
        ? sentence.text
        : `${sentence.text} · ${sentence.title}`,
  };
}

/** The card's "Reported by agents" block. */
export const ROLE_REPORTS_TITLE = "Reported by agents";

export const ROLE_REPORTS_EMPTY = "No reports yet.";

/** The card shows at most five version lines; the rest are in the history. */
export const ROLE_REPORT_VERSION_LIMIT = 5;

export function roleReportsMoreText(count: number): string {
  return `and ${count} more ${count === 1 ? "version" : "versions"} in Report history`;
}

/**
 * The card's one-line form of `roleReportSentence`: the same counts in the
 * same fixed order, in the fewest words that stay true ("2 same version", not
 * "2 on the same version as this computer"). The long sentence is the line's
 * `title`, so the short form abbreviates and never replaces it.
 */
export function roleReportShortSentence(summary: RoleReportSummary): string {
  const parts: string[] = [reportCountText(summary.total)];
  if (summary.sameAsHere > 0) parts.push(`${summary.sameAsHere} same version`);
  if (summary.earlier > 0) parts.push(`${summary.earlier} earlier`);
  if (summary.newer > 0) parts.push(`${summary.newer} newer`);
  if (summary.other > 0) {
    parts.push(
      `${summary.other} on ${summary.other === 1 ? "another version" : "other versions"}`,
    );
  }
  if (summary.unknown > 0) {
    parts.push(`${summary.unknown} no version reported`);
  }
  return parts.join(" · ");
}

/** How many of a role's reports the provenance fold contradicted. */
export function roleDisputedReportCount(summary: RoleReportSummary): number {
  return summary.versions.reduce(
    (total, version) => total + version.provenance.disputed,
    0,
  );
}

/** The word beside a disputed count, wherever one is shown. */
export const DISPUTED_WORD = "disputed";

/** `2 unconfirmed` — reports whose sender could not be confirmed. */
export function unconfirmedCountText(count: number): string {
  return `${count} unconfirmed`;
}

/** `1 disputed` — reports the provenance fold contradicted. */
export function disputedCountText(count: number): string {
  return `${count} ${DISPUTED_WORD}`;
}

/**
 * A managed agent's own status word, as the record reports it. `undefined` is
 * a fourth case and never folded into "stopped": the agent is known here by
 * name and role, and its run state is simply not something this view was
 * told (a relay-discovered agent).
 */
export function agentStatusWord(
  status: RoleAgentChip["status"] | undefined,
): string {
  switch (status) {
    case "running":
      return "running";
    case "deployed":
      return "deployed";
    case "stopped":
      return "stopped";
    case "not_deployed":
      return "not deployed";
    default:
      return AGENT_STATUS_NOT_REPORTED;
  }
}

/** What a chip says when nothing told this view the agent's run state. */
export const AGENT_STATUS_NOT_REPORTED = "status not reported here";

/** What the card's leading dot means, in words, for its `aria-label`. */
export function roleActivityLabel(
  activity: "running" | "idle" | "none",
): string {
  switch (activity) {
    case "running":
      return "A session in this role is running";
    case "idle":
      return "This role has open sessions, none running";
    case "none":
      return "No open sessions in this role";
  }
}

/** The summary strip's four labels, plural-correct. */
export function rolesCountLabel(count: number): string {
  return count === 1 ? "role" : "roles";
}

export function agentsCountLabel(count: number): string {
  return count === 1 ? "agent" : "agents";
}

export function openSessionsCountLabel(count: number): string {
  return count === 1 ? "open session" : "open sessions";
}

export function reportsCountLabel(count: number): string {
  return count === 1 ? "report" : "reports";
}

/**
 * A reported row's relation to this machine's packs checkout — the revision
 * comparison's own vocabulary, plus the two outcomes it never answers
 * (`different-source`, `incomplete`). Final wording from the design: nothing
 * here claims a machine is "current" without a matching relation, and
 * `earlier`/`later` always disclose the commit count rather than rounding it
 * away. Used inside Technical details, and as the `title` of the short form
 * below.
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

/**
 * The same relation in the card's voice: lower-case, short, and without the
 * word "revision". Every distance the long form discloses is kept — an
 * unknown distance says so rather than being dropped.
 */
export function revisionRelationShortText(
  relation: ReportedRolePackRelation,
  behind: number | null,
  ahead: number | null,
): string {
  switch (relation) {
    case "current":
      return "same version as here";
    case "earlier":
      return behind === null
        ? "earlier, distance unknown"
        : `earlier, ${behind} behind`;
    case "later":
      return ahead === null
        ? "newer than here, distance unknown"
        : `newer than here, ${ahead} ahead`;
    case "unrelated":
      return "different history";
    case "unknown-here":
      return "version unknown here";
    case "different-source":
      return "different source";
    case "shipped-differs":
      return "built-in defaults, other app version";
    case "incomplete":
      return "version not reported";
  }
}

/** `reported 5m ago`, or the honest "time not reported" when `ageSeconds` is `null`. */
export function reportedAgoText(ageSeconds: number | null): string {
  return ageSeconds === null
    ? "time not reported"
    : `reported ${formatCoordinationAge(ageSeconds)} ago`;
}

/** The newest report in a version group: `latest just now` / `latest 2m ago`. */
export function reportLatestText(ageSeconds: number | null): string {
  if (ageSeconds === null) return "latest time not reported";
  return ageSeconds < 60
    ? "latest just now"
    : `latest ${formatCoordinationAge(ageSeconds)} ago`;
}

/** `1 report` / `4 reports`. */
export function reportCountText(count: number): string {
  return `${count} ${count === 1 ? "report" : "reports"}`;
}

/** The one sentence a non-current, non-terminal reported row adds. */
export const ADOPTION_KEEPS_UNTIL_NEXT_GENERATION =
  "Keeps this revision until its next launch or resume.";

/**
 * The Revision-snapshots section's own subtitle (final copy, per review).
 * Deliberately never says a commissioned label proves which role
 * instructions ran — only that Beekeeper checked who sent the report. Lives
 * inside Technical details, where the protocol vocabulary is allowed.
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

/**
 * The card's word for the same check: what Beekeeper could say about who
 * sent the report, never about what the agent ran. The long label goes in
 * the line's `title`.
 */
const PROVENANCE_SHORT_TEXT: Record<RolePackProvenanceState, string> = {
  commissioned: "sender confirmed",
  "proof-unavailable": "sender unconfirmed",
  disputed: "disputed",
};

export function provenanceShortText(state: RolePackProvenanceState): string {
  return PROVENANCE_SHORT_TEXT[state];
}

/** A version group whose reports were not all checked the same way. */
export const PROVENANCE_MIXED_SHORT = "senders checked differently";

/** A version group with no provenance counts at all — disclosed, not assumed. */
export const PROVENANCE_NONE_SHORT = "sender checks not recorded";

export type RoleReportProvenanceCounts = {
  commissioned: number;
  unavailable: number;
  disputed: number;
};

/**
 * One phrase for a version group's provenance, plus the long labels and
 * their counts for the tooltip. A group with two different outcomes says
 * they differ rather than reporting the more comfortable one.
 */
export function roleReportProvenanceText(counts: RoleReportProvenanceCounts): {
  text: string;
  title: string;
} {
  const present = (
    [
      ["commissioned", counts.commissioned],
      ["proof-unavailable", counts.unavailable],
      ["disputed", counts.disputed],
    ] as const
  ).filter(([, count]) => count > 0);
  const title = present
    .map(([state, count]) => `${PROVENANCE_STATE_LABEL[state]}: ${count}`)
    .join(" · ");
  if (present.length === 0) {
    return { text: PROVENANCE_NONE_SHORT, title: PROVENANCE_NONE_SHORT };
  }
  if (present.length === 1) {
    const state = present[0]?.[0];
    return {
      text:
        state === undefined
          ? PROVENANCE_NONE_SHORT
          : provenanceShortText(state),
      title,
    };
  }
  return { text: PROVENANCE_MIXED_SHORT, title };
}

/**
 * One reported-version line on a card:
 * `9f2e1d0c · same version as here · 2 reports · latest just now · sender
 * confirmed`. A report that named no version leads with "version not
 * reported" instead of a sha it does not have.
 */
export function roleReportVersionLine(input: {
  sha: string | null;
  relation: ReportedRolePackRelation;
  behind: number | null;
  ahead: number | null;
  count: number;
  latestAgeSeconds: number | null;
  provenance: RoleReportProvenanceCounts;
}): { text: string; title: string } {
  const relationShort = revisionRelationShortText(
    input.relation,
    input.behind,
    input.ahead,
  );
  const provenance = roleReportProvenanceText(input.provenance);
  const lead =
    input.sha === null ? [relationShort] : [shaText(input.sha), relationShort];
  return {
    text: [
      ...lead,
      reportCountText(input.count),
      reportLatestText(input.latestAgeSeconds),
      provenance.text,
    ].join(" · "),
    title: [
      revisionRelationText(input.relation, input.behind, input.ahead),
      provenance.title,
      ...(isCommitSha(input.sha) ? [input.sha] : []),
    ].join(" · "),
  };
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

/** The always-visible line above Technical details when nothing is uncertain. */
export const ROLES_UNCERTAINTY_NONE =
  "Nothing in this view is reported as missing or unconfirmed.";

/** Technical details and its three named groups. */
export const ROLES_TECHNICAL_DETAILS_TITLE = "Technical details";

export const ROLES_DIAGNOSTICS_SOURCE_TITLE = "Where instructions come from";

export const ROLES_DIAGNOSTICS_CHECKS_TITLE = "How Beekeeper checks reports";

export function reportHistorySummary(count: number): string {
  return `Report history (${count})`;
}
