import type {
  TeamReadinessFact,
  TeamReadinessResponse,
  TeamReadinessState,
} from "@/shared/api/tauriTeamReadiness";

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
}): TeamReadinessLaunchGate {
  if (input.projectRef === null) return { allowed: true, reason: null };
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
