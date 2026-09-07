import type {
  TeamReadinessFact,
  TeamReadinessResponse,
  TeamReadinessState,
} from "@/shared/api/tauriTeamReadiness";
import {
  isNewCodingSessionTargetReady,
  type NewCodingSessionTarget,
} from "./newCodingSessionModel";

export type TeamReadinessLaunchGate = {
  allowed: boolean;
  reason: string | null;
};

function factReason(fact: TeamReadinessFact): string {
  return fact.remedy ? `${fact.summary} ${fact.remedy}` : fact.summary;
}

const FIRST_SESSION_AWAITING_CODES = new Set([
  "CATALOG_AWAITING_FIRST_SESSION",
  "RELAY_UNOBSERVED",
]);

export function normalizeTeamReadinessRoles(
  roles: readonly string[],
): string[] {
  return [
    ...new Set(
      roles
        .map((role) => role.trim().toLowerCase())
        .filter((role) => role.length > 0),
    ),
  ].sort();
}

function firstFactReason(
  readiness: TeamReadinessResponse,
  state: "blocked" | "unknown" | "awaiting_first_session",
): string | null {
  const fact = readiness.facts.find((entry) => entry.state === state);
  return fact ? factReason(fact) : null;
}

/** Fail closed for project team launches, while leaving non-project launches alone. */
export function teamReadinessLaunchGate(input: {
  projectRef: string | null;
  loading: boolean;
  error: string | null;
  readiness: TeamReadinessResponse | null;
  /** The exact target selected by the create picker; omitted outside creation UI. */
  runtimeTarget?: NewCodingSessionTarget | null;
  /**
   * Whether the launch uses the project's role packs at all. A session led
   * by the person, with no seats, needs no prepared roles: the checkout,
   * the supervised provider and the role packs are what *seats* run on. When
   * `false` the gate is open and the readiness facts are informational.
   * Defaults to `true` so every existing caller keeps its gate.
   */
  useRoles?: boolean;
}): TeamReadinessLaunchGate {
  if (input.projectRef === null) return { allowed: true, reason: null };
  if (input.useRoles === false) return { allowed: true, reason: null };
  if (input.loading) {
    return {
      allowed: false,
      reason: "Checking whether this project is prepared for a team.",
    };
  }
  if (input.error !== null) {
    return {
      allowed: false,
      reason: `Team readiness is unknown: ${input.error}`,
    };
  }
  if (input.readiness === null) {
    return {
      allowed: false,
      reason:
        "Team readiness is unknown because this project has not been checked.",
    };
  }
  const blockedFactReason = firstFactReason(input.readiness, "blocked");
  if (
    input.readiness.status === "blocked" ||
    input.readiness.blockingCodes.length > 0 ||
    blockedFactReason !== null
  ) {
    return {
      allowed: false,
      reason:
        blockedFactReason ??
        "Team Readiness reports a blocking host fact. Resolve it, then re-read readiness.",
    };
  }
  const unknownFactReason = firstFactReason(input.readiness, "unknown");
  if (
    input.readiness.status === "unknown" ||
    input.readiness.unknownCodes.length > 0 ||
    unknownFactReason !== null
  ) {
    return {
      allowed: false,
      reason:
        unknownFactReason ??
        "Team Readiness contains an unknown host fact. Resolve it, then re-read readiness.",
    };
  }
  const awaitingEvidence = [
    ...new Set([
      ...input.readiness.awaitingCodes,
      ...input.readiness.facts
        .filter((fact) => fact.state === "awaiting_first_session")
        .map((fact) => fact.code),
    ]),
  ];
  const unsupportedAwaiting = awaitingEvidence.filter(
    (code) => !FIRST_SESSION_AWAITING_CODES.has(code),
  );
  const unsupportedAwaitingFact = input.readiness.facts.find(
    (fact) =>
      fact.state === "awaiting_first_session" &&
      unsupportedAwaiting.includes(fact.code),
  );
  if (
    unsupportedAwaiting.length > 0 ||
    (input.readiness.status === "ready" && awaitingEvidence.length > 0)
  ) {
    return {
      allowed: false,
      reason:
        (unsupportedAwaitingFact
          ? factReason(unsupportedAwaitingFact)
          : null) ??
        `Team Readiness is awaiting unsupported evidence: ${unsupportedAwaiting.join(", ") || awaitingEvidence.join(", ")}.`,
    };
  }
  if (
    input.readiness.status === "awaiting_first_session" &&
    awaitingEvidence.length === 0
  ) {
    return {
      allowed: false,
      reason:
        "Team Readiness says awaiting first session but names no supported awaiting fact.",
    };
  }
  if (
    input.readiness.readyForFirstSession &&
    input.readiness.provider.provisioned &&
    input.readiness.provider.process === "live"
  ) {
    if (input.runtimeTarget === null) {
      return {
        allowed: false,
        reason:
          "No installed and authenticated coding-session runtime target is available on this computer.",
      };
    }
    if (
      input.runtimeTarget !== undefined &&
      !isNewCodingSessionTargetReady(input.runtimeTarget)
    ) {
      return {
        allowed: false,
        reason:
          input.runtimeTarget.availability?.hint ??
          "The selected coding-session runtime is not ready.",
      };
    }
    return { allowed: true, reason: null };
  }
  if (
    input.readiness.readyForFirstSession &&
    (!input.readiness.provider.provisioned ||
      input.readiness.provider.process !== "live")
  ) {
    return {
      allowed: false,
      reason:
        "Team Readiness reported no live provider on this computer. Use Prepare, then re-read readiness.",
    };
  }
  const fact =
    input.readiness.facts.find((entry) => entry.state === "blocked") ??
    input.readiness.facts.find((entry) => entry.state === "unknown");
  return {
    allowed: false,
    reason: fact
      ? factReason(fact)
      : "This project is not prepared for its first team session. Re-read Team Readiness for the exact remedy.",
  };
}

export function groupTeamReadinessFacts(
  facts: readonly TeamReadinessFact[],
): Record<TeamReadinessState, TeamReadinessFact[]> {
  return {
    blocked: facts.filter((fact) => fact.state === "blocked"),
    unknown: facts.filter((fact) => fact.state === "unknown"),
    awaiting_first_session: facts.filter(
      (fact) => fact.state === "awaiting_first_session",
    ),
    limited: facts.filter((fact) => fact.state === "limited"),
    ready: facts.filter((fact) => fact.state === "ready"),
  };
}

/** One discovered role pack, as `scan_project_role_packs_directory` returns it. */
export type TeamReadinessRolePack = {
  role: string;
  defaultName: string;
  installed: boolean;
};

/** Which packs this launch confirms, and what it says about the rest. */
export type TeamReadinessPrepareScope = {
  /** The packs whose names this launch actually asks about, role-sorted. */
  confirm: TeamReadinessRolePack[];
  /** Roles this launch names that the folder has no pack for. */
  missingRoles: string[];
  /** Roles refreshed without being asked about, role-sorted. */
  otherRoles: string[];
  otherCount: number;
  /** §1g's line, or null when there is nothing else to say. */
  otherLine: string | null;
};

/**
 * Scope Prepare to the seats this launch actually names.
 *
 * Live run 2, 10:36 (finding 15): a two-seat launch put **six** name fields on
 * screen under "Confirm every refreshed role name", because the installer
 * refreshes every pack under `personas/roles` and the screen therefore asked
 * about every pack. It read as role selection and was pack maintenance, and
 * the two seats a person was actually launching were lost in it.
 *
 * The install is unchanged — every discovered pack is still refreshed, and
 * every one still gets its stored name, so nothing silently loses a name. What
 * changes is the question: a launch asks about its own lead and bench, and
 * says in one line how many other packs it refreshed. Refreshed silently is
 * not refreshed secretly.
 */
export function teamReadinessPrepareScope(input: {
  packs: readonly TeamReadinessRolePack[];
  selectedRoles: readonly string[];
}): TeamReadinessPrepareScope {
  const selected = new Set(normalizeTeamReadinessRoles(input.selectedRoles));
  const byRole = (left: { role: string }, right: { role: string }) =>
    left.role.localeCompare(right.role);
  const confirm = input.packs
    .filter((pack) => selected.has(pack.role.trim().toLowerCase()))
    .slice()
    .sort(byRole);
  const others = input.packs
    .filter((pack) => !selected.has(pack.role.trim().toLowerCase()))
    .slice()
    .sort(byRole);
  const found = new Set(
    input.packs.map((pack) => pack.role.trim().toLowerCase()),
  );
  return {
    confirm,
    missingRoles: [...selected].filter((role) => !found.has(role)).sort(),
    otherRoles: others.map((pack) => pack.role),
    otherCount: others.length,
    // §1g freezes the plural form; the singular is spelled out rather than
    // printed as "1 other packs were", which would be the kind of small lie
    // that makes a reader distrust the rest of the screen.
    otherLine:
      others.length === 0
        ? null
        : others.length === 1
          ? "1 other pack was refreshed and is not part of this launch."
          : `${others.length} other packs were refreshed and are not part of this launch.`,
  };
}
