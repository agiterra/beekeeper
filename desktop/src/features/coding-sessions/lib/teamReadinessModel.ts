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

/**
 * The Unknown fact that is *required* for the first session, or null when
 * every Unknown fact is a `wire`-scope one.
 *
 * Backend readiness already draws this line: `readyForFirstSession` is
 * computed over `local`-scope facts only (`team_readiness.rs`,
 * `response_from_facts`), because "first session" means this computer's own
 * inventory — a recorded checkout, staged role packs, a live local
 * provider — never a fact this computer can only learn by asking the relay
 * for a signed catalog, which does not exist until a first session
 * publishes one. A `wire`-scope Unknown (catalog coverage, trust config) is
 * about *full* readiness and must warn, not block: ledger 137 left
 * `CATALOG_TARGETS_UNCOVERED` (Unknown, wire) blocking Start beside
 * `REGISTRY_UNREADABLE` (Limited, local) for the same absent registry;
 * ledger 140 makes the local/wire split explicit here instead of reading
 * the ambiguous top-level `status` string.
 */
export function teamReadinessRequiredUnknownFact(
  readiness: TeamReadinessResponse | null,
): TeamReadinessFact | null {
  if (!readiness) return null;
  return (
    readiness.facts.find(
      (fact) => fact.state === "unknown" && fact.scope === "local",
    ) ?? null
  );
}

/** A fact about where this project's code lives on this computer. */
function isCheckoutFact(fact: TeamReadinessFact): boolean {
  return fact.category === "checkout" || fact.code.startsWith("CHECKOUT_");
}

/**
 * The gate a launch that does not use roles still passes: the checkout.
 *
 * "Use roles" off bypasses the role and pack facts — a person-led session
 * with no seats runs on none of them — but never the checkout. Every seat a
 * lead later hires gets a worktree cut from the project's recorded folder,
 * and the first RPG Test team session (2026-09-19) got past this gate with
 * roles off, ran in another project's checkout, and could not hire. So with
 * roles off the checkout fact alone is read, and read fail-closed: a
 * readiness that is loading, errored or absent cannot vouch for the folder.
 */
function checkoutOnlyLaunchGate(input: {
  loading: boolean;
  error: string | null;
  readiness: TeamReadinessResponse | null;
}): TeamReadinessLaunchGate {
  if (input.loading) {
    return {
      allowed: false,
      reason: "Checking where this project's code lives on this computer.",
    };
  }
  if (input.error !== null) {
    return {
      allowed: false,
      reason: `This computer could not confirm where this project's code lives: ${input.error}`,
    };
  }
  if (input.readiness === null) {
    return {
      allowed: false,
      reason:
        "This computer could not confirm where this project's code lives because this project has not been checked.",
    };
  }
  const checkoutProblem = input.readiness.facts.find(
    (fact) =>
      isCheckoutFact(fact) &&
      (fact.state === "blocked" || fact.state === "unknown"),
  );
  if (checkoutProblem) {
    return { allowed: false, reason: factReason(checkoutProblem) };
  }
  return { allowed: true, reason: null };
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
   * by the person, with no seats, needs no prepared roles: the supervised
   * provider and the role packs are what *seats* run on. When `false` only
   * the checkout fact gates (`checkoutOnlyLaunchGate`) and the other
   * readiness facts are informational. Defaults to `true` so every existing
   * caller keeps its full gate.
   */
  useRoles?: boolean;
}): TeamReadinessLaunchGate {
  if (input.projectRef === null) return { allowed: true, reason: null };
  if (input.useRoles === false) return checkoutOnlyLaunchGate(input);
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
  const requiredUnknownFact = teamReadinessRequiredUnknownFact(input.readiness);
  if (requiredUnknownFact) {
    return {
      allowed: false,
      reason: factReason(requiredUnknownFact),
    };
  }
  // `unknownCodes` naming a code with no matching fact in `facts` is a
  // malformed or legacy payload this function cannot scope — fail closed
  // rather than guess it was only ever a `wire` one.
  const unknownCodesWithNoFact =
    input.readiness.unknownCodes.length > 0 &&
    !input.readiness.facts.some((fact) => fact.state === "unknown");
  if (unknownCodesWithNoFact) {
    return {
      allowed: false,
      reason:
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

/**
 * What a readiness blocker means, in the words a person acts on.
 *
 * ## Why this file exists
 *
 * The readiness panel used to print `CODE · summary. Remedy: …`, one line per
 * fact. Every line was true, and the screen was still unusable: ledger 135(c)
 * has a founding form blocked on `ROLE_PACKS_MISSING` ("Restore
 * personas/roles") and `REGISTRY_UNREADABLE` (`team/model-registry.yaml`) for
 * a project whose seats were going to be staged from a repository on the
 * relay and would never read either path. The operator's way out was to untick
 * "Use roles". A control that reads like a diagnosis of the wrong thing is a
 * control people learn to switch off.
 *
 * So each code a person can actually hit gets a heading and a sentence saying
 * what it means and what to do. The code and the host's own sentence are still
 * shown underneath — this narrows nothing and hides nothing; the host remains
 * the authority on the fact, and this file is the authority on nothing but
 * the English.
 *
 * A code with no entry here renders exactly as before. A missing translation
 * must never swallow a fact.
 */

export type TeamReadinessBlockerCopy = {
  /** The short line a person scans. Sentence case, no trailing period. */
  title: string;
  /** What to do about it, in one sentence. */
  action: string;
};

const COPY: Record<string, TeamReadinessBlockerCopy> = {
  CHECKOUT_NOT_RECORDED: {
    title: "This computer does not know where this project's code lives",
    action:
      "Agents get their own worktree cut from that folder, so a session cannot start without it. Finish repository setup (Project settings → Packs) clones the project's repository and records it; or pick the folder below, or set it in Project settings → This computer.",
  },
  CHECKOUT_UNAVAILABLE: {
    title: "The recorded folder is not there any more",
    action:
      "It was moved, renamed, or is on a disk that is not mounted. Point this project at where the checkout is now.",
  },
  CHECKOUT_STORE_UNREADABLE: {
    title: "This computer's record of project folders could not be read",
    action: "Choose this project's folder again to rewrite the record.",
  },
  ROLE_PACKS_MISSING: {
    title: "Nothing says what this project's roles are",
    action:
      "Roles come from a packs repository the project names, or from a personas/roles folder in the checkout. This project has neither, so there is no prompt to seat an agent with.",
  },
  ROLE_PACKS_SOURCE_UNAVAILABLE: {
    title: "This project's packs repository could not be read",
    action:
      "Every seat is staged from it, so a hire would be refused for the same reason. Check the repository and the commit the project names under Project settings → Roles.",
  },
  ROLE_PACKS_EMPTY: {
    title: "The role-pack folder holds no usable roles",
    action:
      "Each role is a directory with a persona that declares it. Add at least one, then scan again.",
  },
  ROLE_PACKS_UNREADABLE: {
    title: "The role-pack folder could not be read",
    action: "Check the folder's permissions and contents, then scan again.",
  },
  SELECTED_ROLE_UNAVAILABLE: {
    title: "This session asks for a role nothing can supply",
    action:
      "Either pick a role that exists for this project, or add that role to wherever this project's packs come from.",
  },
  SELECTED_ROLE_NOT_INSTALLED: {
    title: "A role this session names has no agent on this computer yet",
    action: "Use Prepare below to create it and confirm its name.",
  },
  SELECTED_ROLE_WRONG_PROJECT: {
    title: "A role this session names is installed from another project",
    action:
      "Use Prepare below to install this project's own pack for it, so the agent runs this project's prompt.",
  },
  SELECTED_ROLE_PACK_DIRTY: {
    title: "A role's installed prompt no longer matches its pack",
    action:
      "Someone edited the pack after the agent was installed. Use Prepare below to refresh it.",
  },
  SELECTED_ROLE_PACK_STATE_UNKNOWN: {
    title: "A role's installed prompt cannot be traced to a pack",
    action:
      "Nothing records which version it came from. Use Prepare below to reinstall it from the pack.",
  },
  SELECTED_ROLE_KEY_UNVERIFIED: {
    title: "A role's signing key has not been checked on this computer",
    action:
      "Use Prepare below to start that identity, which is what proves the key can be read.",
  },
  REGISTRY_UNREADABLE: {
    title: "This project pins no provider or model targets",
    // A project's registry lives in its agents repository first and its
    // checkout second — the hire host's order (ledger 180). Saying only
    // "the checkout" told the founder of a project that HAD one to add a
    // second copy in the wrong place (ledger 207(1)); the host's own remedy,
    // shown under this, names the place for this project.
    action:
      "A project's registry is model-registry.yaml in its agents repository, or team/model-registry.yaml in its checkout. With neither, each role pack's own runtime and model is used.",
  },
  REGISTRY_INVALID: {
    title: "This project's model registry could not be understood",
    action:
      "Repair it as a version 1 registry; the fact below names the file that was read.",
  },
  PROVIDER_NOT_RUNNING: {
    title: "No coding-session provider is running on this computer",
    action: "Use Prepare below to provision and start one.",
  },
  PROVIDER_IN_BACKOFF: {
    title: "The provider is supervised but has no live process",
    action: "Wait for it to recover, or restart it from Settings.",
  },
  PROJECT_ACTIONS_NOT_DELEGABLE: {
    title:
      "This session's lead will not be able to publish this project's actions",
    action:
      "Publishing an action and starting a manual run of one are admitted for this project's owners, and the person founding this session is not one of them. Either a project owner signs the delegation for this session's lead, or found the session as an owner.",
  },
  PROJECT_ACTIONS_AUTHORITY_UNKNOWN: {
    title: "Whether the lead may publish this project's actions is unknown",
    action:
      "The relay could not answer who owns this project, so this is unconfirmed rather than refused. Start is not blocked; if the lead is refused later, a project owner signs the delegation for it.",
  },
  RUNTIME_UNAVAILABLE: {
    title: "No coding runtime is installed and signed in here",
    action:
      "Install and authenticate at least one runtime (Claude Code or Codex), then re-read this panel.",
  },
};

/** The plain-language copy for a readiness code, or `null` when it has none. */
export function teamReadinessBlockerCopy(
  code: string,
): TeamReadinessBlockerCopy | null {
  return COPY[code] ?? null;
}

/** Every code this file translates, for the test that keeps it honest. */
export function translatedTeamReadinessCodes(): string[] {
  return Object.keys(COPY).sort();
}
