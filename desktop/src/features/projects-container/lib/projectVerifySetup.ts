/**
 * Project setup's one standing consent (ledger 248, 252; the plan's
 * "Approval" ruling, spec § 5.4).
 *
 * Creation seeds an active `verify` action, and the host command
 * `project_verify_setup`
 * (`desktop/src-tauri/src/managed_agents/project_verify_setup.rs`) publishes
 * it — nothing more. The "Approve and allow future runs" click answers
 * directly with a standing grant (`grantStandingApproval`,
 * `desktop/src-tauri/src/commands/workflows.rs` `grant_standing_approval`)
 * bound to `(workflowId, definitionHash)`, with no run and no kind:46010 in
 * between.
 *
 * Control run 6 (2026-09-24) found the prior shape started a real run at the
 * code repository's empty seed commit purely to park it on a synthetic
 * approval gate and manufacture something for the click to answer — a run
 * on a provider setup never starts, so it sat unclaimed and then ran red on
 * a commit with no tests.
 */
import { invokeTauri } from "@/shared/api/tauri";

/** The host command this module calls. */
export const PROJECT_VERIFY_SETUP_COMMAND = "project_verify_setup";

/**
 * The creation form's default verify command — the standard-library test
 * runner over `tests/`, `buzz_persona::seed::DEFAULT_VERIFY_COMMAND`.
 */
export const DEFAULT_VERIFY_COMMAND_TEXT =
  "python3 -m unittest discover -s tests";

/** Everything `project_verify_setup` reports; `null` = step did not happen. */
export type ProjectVerifySetupResult = {
  workflowId: string | null;
  channelId: string | null;
  definitionHash: string | null;
  command: string[];
  publishEventId: string | null;
  channelReused: boolean;
  /** The code repository's seed commit — informational; no run is bound to it. */
  checkout: string | null;
  /** The step that stopped setup, in the host's words. */
  error: string | null;
};

function optionalString(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

/** Read the host's answer, refusing a shape this reader cannot vouch for. */
export function decodeProjectVerifySetup(
  value: unknown,
): ProjectVerifySetupResult {
  const record = (
    typeof value === "object" && value !== null ? value : {}
  ) as Record<string, unknown>;
  const ok =
    optionalString(record.workflowId) &&
    optionalString(record.channelId) &&
    optionalString(record.definitionHash) &&
    Array.isArray(record.command) &&
    record.command.every((arg) => typeof arg === "string") &&
    optionalString(record.publishEventId) &&
    typeof record.channelReused === "boolean" &&
    optionalString(record.checkout) &&
    optionalString(record.error);
  if (!ok) {
    throw new Error(
      "native project-verify-setup adapter returned a malformed response",
    );
  }
  return record as unknown as ProjectVerifySetupResult;
}

/** Publish the seeded verify and start the run that carries the question. */
export async function projectVerifySetup(input: {
  projectRef: string;
  /** Names the project's sessions channel, where `verify` is filed. */
  projectName: string;
  actionsYml: string;
  checkout: string;
}): Promise<ProjectVerifySetupResult> {
  return decodeProjectVerifySetup(
    await invokeTauri(PROJECT_VERIFY_SETUP_COMMAND, input),
  );
}

/**
 * The form's text as argv: whitespace separates arguments, single or double
 * quotes keep one together. Never run through a shell. Blank → `null`, which
 * the host reads as the default.
 */
export function parseVerifyCommand(text: string): string[] | null {
  const args: string[] = [];
  let current = "";
  let quote: '"' | "'" | null = null;
  let started = false;
  for (const char of text) {
    if (quote !== null) {
      if (char === quote) quote = null;
      else current += char;
      continue;
    }
    if (char === '"' || char === "'") {
      quote = char;
      started = true;
    } else if (/\s/.test(char)) {
      if (started) args.push(current);
      current = "";
      started = false;
    } else {
      current += char;
      started = true;
    }
  }
  if (started) args.push(current);
  return args.length > 0 ? args : null;
}

/**
 * Why the consent question cannot be asked, or `null` when setup published
 * the definition and a standing grant may be requested for it.
 */
export function verifyConsentUnavailable(
  verify: ProjectVerifySetupResult | null,
  error: string | null,
): string | null {
  if (error !== null) return `Verify setup did not run: ${error}`;
  if (verify === null)
    return "The verify action was not published, because this computer did not seed and push the agents repository.";
  if (verify.error !== null) return verify.error;
  if (verify.workflowId === null || verify.definitionHash === null)
    return "The verify action was accepted but the relay named no definition to approve.";
  return null;
}
