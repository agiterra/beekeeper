import { normalizeProjectCoordinate } from "@/shared/lib/projectAgentAssociation";

/**
 * Read-only views over a kind:30620 definition as the Actions tab needs
 * them. Deliberately a copy of the few lines the Workflows page uses rather
 * than an import: a feature module does not import another feature module.
 */

function asRecord(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
}

/** Trigger names as the definition spells them, in the tab's wording. */
const TRIGGER_LABELS: Record<string, string> = {
  message_posted: "Message posted",
  reaction_added: "Reaction added",
  diff_posted: "Diff posted",
  webhook: "Webhook",
  schedule: "Schedule",
  manual: "Manual",
  ref_updated: "Ref updated",
  ci_result: "CI result",
};

/**
 * Whether a definition is one of this project's actions: its `project` is
 * the project's own kind:30621 coordinate. Definitions with no `project`, or
 * another project's, are ordinary channel workflows and stay off the tab.
 */
export function isProjectAction(
  definition: Record<string, unknown>,
  projectRef: string,
): boolean {
  const declared = normalizeProjectCoordinate(
    typeof definition.project === "string" ? definition.project : null,
  );
  const expected = normalizeProjectCoordinate(projectRef);
  return declared !== null && expected !== null && declared === expected;
}

/** `Webhook`, `Schedule · 0 2 * * *`, `Message posted · deploy`, … */
export function actionTriggerSummary(
  definition: Record<string, unknown>,
): string | null {
  const trigger = asRecord(definition.trigger);
  const on = trigger?.on;
  if (!trigger || typeof on !== "string" || on.trim().length === 0) {
    return null;
  }
  const label = TRIGGER_LABELS[on] ?? on;
  const detailKeys: Record<string, string> = {
    message_posted: "filter",
    diff_posted: "filter",
    reaction_added: "emoji",
    ref_updated: "ref",
    ci_result: "check",
  };
  const detailKey = detailKeys[on];
  if (detailKey) {
    const detail = trigger[detailKey];
    return typeof detail === "string" && detail.trim().length > 0
      ? `${label} · ${detail.trim()}`
      : label;
  }
  if (on === "schedule") {
    for (const key of ["cron", "interval"]) {
      const value = trigger[key];
      if (typeof value === "string" && value.trim().length > 0) {
        return `${label} · ${value.trim()}`;
      }
    }
  }
  return label;
}

/** The ids of the definition's `run_on_host` steps, in order. */
export function runOnHostStepIds(
  definition: Record<string, unknown>,
): string[] {
  if (!Array.isArray(definition.steps)) return [];
  const ids: string[] = [];
  for (const raw of definition.steps) {
    const step = asRecord(raw);
    if (step?.action === "run_on_host" && typeof step.id === "string") {
      ids.push(step.id);
    }
  }
  return ids;
}

/** The definition's description, trimmed, or null when it has none. */
export function actionDescription(
  definition: Record<string, unknown>,
): string | null {
  const description = definition.description;
  return typeof description === "string" && description.trim().length > 0
    ? description.trim()
    : null;
}

/**
 * The `run_on_host` steps of a definition that declare `checkout: required`.
 *
 * Lane 184: such a step refuses a run that names no commit, so the Run
 * control must ask for one. An action with none runs in the recorded project
 * directory exactly as it is found, and the control says that plainly instead
 * of implying an isolated checkout.
 */
export function requiredCheckoutStepIds(
  definition: Record<string, unknown>,
): string[] {
  if (!Array.isArray(definition.steps)) return [];
  const ids: string[] = [];
  for (const raw of definition.steps) {
    const step = asRecord(raw);
    if (
      step?.action === "run_on_host" &&
      step.checkout === "required" &&
      typeof step.id === "string"
    ) {
      ids.push(step.id);
    }
  }
  return ids;
}

/**
 * A `run_on_host` step's command, in the form the definition wrote it.
 *
 * Kept as **structure**, never as one line. Lane 203 joined an argv with
 * spaces, so `["sh","-c","a b"]` rendered as `sh -c a b` — four arguments
 * where the definition names three, and the difference is the difference
 * between running `a` and running `a b`. An approver is being asked to let a
 * command run on their own computer; the display must not be able to lie
 * about how it is split (Astra's Wave 2 review, finding 1).
 */
export type HostStepCommand =
  | { form: "argv"; argv: readonly string[] }
  | { form: "shell"; text: string };

/**
 * The command a `run_on_host` step would execute, as the published
 * definition spells it.
 *
 * `null` when the step is not in this definition, is not a `run_on_host`
 * step, or carries no command a reader can render — an approval card shows
 * that as a named absence and withholds Approve, never as an empty line.
 */
export function hostStepCommand(
  definition: Record<string, unknown>,
  stepId: string,
): HostStepCommand | null {
  if (!Array.isArray(definition.steps)) return null;
  for (const raw of definition.steps) {
    const step = asRecord(raw);
    if (step?.id !== stepId || step.action !== "run_on_host") continue;
    const command = step.command;
    if (typeof command === "string" && command.trim().length > 0) {
      return { form: "shell", text: command.trim() };
    }
    if (Array.isArray(command)) {
      const argv = command.filter(
        (part): part is string => typeof part === "string",
      );
      if (argv.length === command.length && argv.length > 0) {
        return { form: "argv", argv };
      }
    }
    return null;
  }
  return null;
}

/**
 * The lines a command is displayed as: one argument per line for an argv,
 * the single line for a shell command.
 *
 * Never a space-joined string. Callers render these as separate rows and
 * show the JSON array beside them, so the split is visible.
 */
export function hostStepCommandLines(
  command: HostStepCommand,
): readonly string[] {
  return command.form === "argv" ? command.argv : [command.text];
}

/** The unambiguous one-line form: a JSON array, or the shell line quoted. */
export function hostStepCommandJson(command: HostStepCommand): string {
  return command.form === "argv"
    ? JSON.stringify(command.argv)
    : JSON.stringify(command.text);
}

/** Whether `value` is a full 40-hex commit sha, as the relay demands. */
export function isFullCommitSha(value: string): boolean {
  return /^[0-9a-f]{40}$/i.test(value.trim());
}
