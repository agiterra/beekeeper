/**
 * The project-creation result, kept until someone dismisses it.
 *
 * # The finding this exists for
 *
 * Ledger 207(5). Creating "Kettle Smoke" on 2026-09-20 did seven things —
 * announced two repositories, seeded both, set the role source, installed
 * eight agents, cloned and recorded the checkout, and put the agents on the
 * roster — and reported all seven in a toast that vanished before the
 * operator could read it, let alone screenshot it. A result you cannot read
 * is a result you cannot trust, and re-deriving it afterwards means asking
 * seven different screens.
 *
 * So the outcome is written down, keyed by project coordinate, and the new
 * project's Overview shows a **Setup** card from it until it is both
 * complete and dismissed. The shape stored is the host's own
 * `ProjectAgentsInitResult` — the same rows the Finish-repository-setup panel
 * prints — never a second, prettier summary that could drift from it.
 */
import {
  describeCheckoutOutcome,
  describeCodeSeedOutcome,
  describeRosterOutcome,
  decodeProjectAgentsInitResult,
  type ProjectAgentsInitResult,
} from "./projectAgentsInit";
import {
  decodeProjectVerifySetup,
  type ProjectVerifySetupResult,
} from "./projectVerifySetup";

const STORAGE_KEY = "beekeeper.project-setup-outcome.v1";

/** One thing creation tried, and how it went. */
export type ProjectSetupStep = {
  id: string;
  /** What was attempted, in the operator's words. */
  label: string;
  /** What happened, in the host's words. */
  detail: string;
  /** A step that did not land. Failures sort first. */
  failed: boolean;
};

/** What is kept for one project. */
export type ProjectSetupOutcome = {
  projectRef: string;
  projectName: string;
  /** ISO-8601, when the create ran. */
  at: string;
  /** The host's report, or `null` when the command itself failed. */
  result: ProjectAgentsInitResult | null;
  /** The command's own words when it threw. */
  error: string | null;
  /**
   * The seeded verify's publication and consent run (ledger 248). Absent in
   * rows written before it existed; `null` when nothing was published.
   */
  verify?: ProjectVerifySetupResult | null;
  /** `project_verify_setup`'s own words when it threw. */
  verifyError?: string | null;
};

/**
 * Every step, failures first and then in the order creation runs them.
 *
 * Each row's `detail` is the host's own field or sentence. Nothing here
 * composes a happier one, and a step the host said nothing about reads as
 * "not reported", never as done.
 */
export function projectSetupSteps(
  outcome: ProjectSetupOutcome,
): ProjectSetupStep[] {
  const result = outcome.result;
  if (!result) {
    return [
      {
        id: "repositories",
        label: "Create the project's repositories",
        detail: outcome.error ?? "the host did not say why",
        failed: true,
      },
    ];
  }
  const steps: ProjectSetupStep[] = [
    {
      id: "code-repo",
      label: "Announce the code repository",
      detail: result.codeRepoExisted
        ? `${result.codeRepoId} already announced`
        : result.codeAnnouncementEventId
          ? `${result.codeRepoId} announced`
          : "not announced",
      failed: !result.codeRepoExisted && !result.codeAnnouncementEventId,
    },
    {
      id: "code-seed",
      label: "Seed the code repository",
      detail: describeCodeSeedOutcome(result),
      failed: !result.codeSeedCommitSha && !result.codeSeedSkipped,
    },
    {
      id: "agents-repo",
      label: "Announce the agents repository",
      detail: result.agentsRepoExisted
        ? `${result.agentsRepoId} already announced`
        : result.agentsAnnouncementEventId
          ? `${result.agentsRepoId} announced`
          : (result.agentsAnnouncementWithdrawalError ?? "not announced"),
      failed: !result.agentsRepoExisted && !result.agentsAnnouncementEventId,
    },
    {
      id: "agents-seed",
      label: "Seed the agents repository with this build's roles",
      detail: result.seedCommitSha
        ? `${result.seedCommitSha.slice(0, 8)} (${result.roles.join(", ")})`
        : result.seedSkipped
          ? "already on the relay"
          : (result.seedError ?? result.pushError ?? "not reached"),
      failed: !result.seedCommitSha && !result.seedSkipped,
    },
    {
      id: "source",
      label: "Set the project's role source",
      detail: result.sourceEventId
        ? `set to ${result.agentsRepoId} on ${result.branch}`
        : result.sourceExisted
          ? "already set"
          : (result.publicationError ?? "not set"),
      failed: !result.sourceEventId && !result.sourceExisted,
    },
    {
      id: "agents",
      label: "Install the project's agents on this computer",
      detail:
        result.agentsInstalled.length > 0
          ? result.agentsInstalled
              .map((agent) => `${agent.name} (${agent.role})`)
              .join(", ")
          : (result.agentsError ?? "none installed"),
      failed: result.agentsInstalled.length === 0,
    },
    {
      id: "checkout",
      label: "Clone the code and record this project's folder",
      detail: describeCheckoutOutcome(result),
      failed: result.checkoutPath === null,
    },
    {
      id: "roster",
      label: "Add the project's agents to its roster",
      detail: describeRosterOutcome(result),
      failed: result.rosterError !== null,
    },
  ];
  // Failures first; within each half, creation's own order is preserved.
  return [
    ...steps.filter((step) => step.failed),
    ...steps.filter((step) => !step.failed),
  ];
}

/** Whether anything is still missing — the host's own verdict, not a count. */
export function projectSetupIncomplete(outcome: ProjectSetupOutcome): boolean {
  return outcome.result === null || !outcome.result.complete;
}

function readAll(): Record<string, unknown> {
  if (typeof localStorage === "undefined") return {};
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return {};
    return parsed as Record<string, unknown>;
  } catch {
    return {};
  }
}

function writeAll(rows: Record<string, unknown>): void {
  if (typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(rows));
  } catch {
    // A full or disabled store loses the card, not the project.
  }
}

/** Keep one project's creation outcome. */
export function storeProjectSetupOutcome(outcome: ProjectSetupOutcome): void {
  const rows = readAll();
  rows[outcome.projectRef] = outcome;
  writeAll(rows);
}

/**
 * Read one project's creation outcome, refusing a shape this reader cannot
 * vouch for — a stored row from an older build is dropped, never rendered
 * half-understood.
 */
export function readProjectSetupOutcome(
  projectRef: string,
): ProjectSetupOutcome | null {
  const row = readAll()[projectRef];
  if (typeof row !== "object" || row === null) return null;
  const record = row as Record<string, unknown>;
  if (
    typeof record.projectRef !== "string" ||
    typeof record.projectName !== "string" ||
    typeof record.at !== "string"
  ) {
    return null;
  }
  let result: ProjectAgentsInitResult | null = null;
  if (record.result !== null && record.result !== undefined) {
    try {
      result = decodeProjectAgentsInitResult(record.result);
    } catch {
      return null;
    }
  }
  return {
    projectRef: record.projectRef,
    projectName: record.projectName,
    at: record.at,
    result,
    error: typeof record.error === "string" ? record.error : null,
    verify: readStoredVerify(record.verify),
    verifyError:
      typeof record.verifyError === "string" ? record.verifyError : null,
  };
}

function readStoredVerify(value: unknown): ProjectVerifySetupResult | null {
  if (value === null || value === undefined) return null;
  try {
    return decodeProjectVerifySetup(value);
  } catch {
    return null;
  }
}

/** Forget one project's creation outcome — the card's Dismiss. */
export function dismissProjectSetupOutcome(projectRef: string): void {
  const rows = readAll();
  if (!(projectRef in rows)) return;
  delete rows[projectRef];
  writeAll(rows);
}
