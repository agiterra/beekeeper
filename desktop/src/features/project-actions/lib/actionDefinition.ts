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
