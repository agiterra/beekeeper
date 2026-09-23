/**
 * Project setup's one standing consent (ledger 248; the plan's "Approval"
 * ruling, spec § 5.4).
 *
 * Creation seeds an active `verify` action, and the host command
 * `project_verify_setup`
 * (`desktop/src-tauri/src/managed_agents/project_verify_setup.rs`) publishes
 * it and starts one run at the code repository's seed commit. The relay
 * parks that run on its synthetic approval gate and publishes the existing
 * kind:46010; the Setup card renders it with the existing approval card, and
 * its "allow future runs of this exact definition" answer is the existing
 * hash-bound grant. Nothing new is stored, and nothing here decides what the
 * card may offer: the card itself keeps Approve disabled until the run, its
 * bound definition and the approver all resolve.
 */
import { invokeTauri } from "@/shared/api/tauri";

import { readHostStepApprovalRequest } from "@/features/project-actions/lib/hostStepApproval";

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
  checkout: string | null;
  runId: string | null;
  triggerEventId: string | null;
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
    optionalString(record.runId) &&
    optionalString(record.triggerEventId) &&
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

type RelayEventLike = {
  kind: number;
  tags: readonly (readonly string[])[];
  content: string;
};

/** The kind:46010 that parks setup's own run, or `null`. */
export function findApprovalRequestForRun<T extends RelayEventLike>(
  events: readonly T[],
  runId: string | null,
): T | null {
  if (runId === null) return null;
  return (
    events.find(
      (event) => readHostStepApprovalRequest(event)?.runId === runId,
    ) ?? null
  );
}

/**
 * Why the consent question cannot be asked, or `null` when setup published
 * the definition and started the run it rides on.
 */
export function verifyConsentUnavailable(
  verify: ProjectVerifySetupResult | null,
  error: string | null,
): string | null {
  if (error !== null) return `Verify setup did not run: ${error}`;
  if (verify === null)
    return "The verify action was not published, because this computer did not seed and push the agents repository.";
  if (verify.error !== null) return verify.error;
  if (verify.runId === null || verify.channelId === null)
    return "The verify run was accepted but the relay named no run to approve.";
  return null;
}
