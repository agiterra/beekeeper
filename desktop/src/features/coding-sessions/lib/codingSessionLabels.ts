/**
 * Human-facing labels for the provider-neutral runtime/provider identifiers a
 * signed session carries.
 *
 * These live apart from the create-flow model so the workspace can label a
 * session it merely reads without pulling in the whole creation surface.
 */

import { splitCodingSessionModelId } from "./codingSessionModelChoice";

export function formatCodingSessionRuntimeLabel(runtime: string): string {
  const normalized = runtime.trim().toLowerCase().replaceAll("_", "-");
  if (normalized === "claude-agent-acp" || normalized === "claude-code") {
    return "Claude Code";
  }
  if (normalized === "codex-acp") return "Codex";
  return runtime
    .split(/[-_\s]+/)
    .filter(Boolean)
    .map((token) => {
      const lower = token.toLowerCase();
      if (lower === "claude") return "Claude";
      if (lower === "codex") return "Codex";
      if (lower === "cc") return "Claude Code";
      if (lower === "gpt") return "GPT";
      return `${token.slice(0, 1).toUpperCase()}${token.slice(1)}`;
    })
    .join(" ");
}

export function formatCodingSessionProviderLabel(input: {
  runtime: string;
  providerInstanceRef: string;
}): string {
  const runtimeLabel = formatCodingSessionRuntimeLabel(input.runtime);
  const runtimeTokens = new Set(
    input.runtime
      .toLowerCase()
      .split(/[-_\s]+/)
      .filter(Boolean),
  );
  const instanceLabel = input.providerInstanceRef
    .split(/[-_\s]+/)
    .filter((token) => {
      const lower = token.toLowerCase();
      return (
        lower.length > 0 &&
        lower !== "provider" &&
        !runtimeTokens.has(lower) &&
        !(lower === "cc" && runtimeTokens.has("claude"))
      );
    })
    .map(
      (token) =>
        `${token.slice(0, 1).toUpperCase()}${token.slice(1).toLowerCase()}`,
    )
    .join(" ");
  return instanceLabel.length > 0
    ? `${runtimeLabel} · ${instanceLabel}`
    : runtimeLabel;
}

export type CodingSessionModelDisplay = {
  model: string;
  thinking: string | null;
  context: string | null;
};

/** Decode the adapter's packed model id into human-facing dimensions. */
export function formatCodingSessionModelDisplay(
  modelId: string,
): CodingSessionModelDisplay {
  const parsed = splitCodingSessionModelId(modelId);
  return {
    model: parsed.model,
    thinking: parsed.thinking ? titleCaseLabel(parsed.thinking) : null,
    context: parsed.context ? parsed.context.toUpperCase() : null,
  };
}

/** A compact label for places that cannot render the dimensions separately. */
export function formatCodingSessionModelSummary(modelId: string): string {
  const display = formatCodingSessionModelDisplay(modelId);
  return [display.model, display.thinking, display.context]
    .filter((value): value is string => Boolean(value))
    .join(" · ");
}

function titleCaseLabel(value: string): string {
  return `${value.slice(0, 1).toUpperCase()}${value.slice(1).toLowerCase()}`;
}

/**
 * How one execution names itself.
 *
 * A human-created execution is its runtime and model — there is nothing else
 * to say about it. A *seated* execution is an agent doing a job, so the agent
 * and the job lead, and the runtime it happens to be running on is demoted to
 * the secondary line (the hover in the participant rail, the subtitle in the
 * header). Getting this backwards was the honest complaint about the crew
 * surface: three chips reading `Claude · sonnet` for three different seats.
 */
export type CodingSessionExecutionLabel = {
  /** The line that identifies the execution. */
  primary: string;
  /** Runtime/model, when it is not already the primary line. */
  secondary: string | null;
};

/**
 * Label an execution, seated or not.
 *
 * `agentDisplayName` is whatever profile lookup resolved for the actor; when
 * it is null the caller has not resolved a name yet, and the label falls back
 * to the role alone rather than inventing one — a seat labelled with a
 * truncated key reads as a bug, and the role is the honest part.
 */
export function formatCodingSessionExecutionLabel(input: {
  /** Absent, null, or empty all mean the same thing: no seat. */
  agentRef: string | null | undefined;
  role: string | null | undefined;
  agentDisplayName?: string | null;
  runtime: string | null | undefined;
  model: string | null | undefined;
}): CodingSessionExecutionLabel {
  const runtimeLabel = input.runtime
    ? formatCodingSessionRuntimeLabel(input.runtime)
    : null;
  const model = nonEmpty(input.model);
  const runtimeSummary =
    runtimeLabel && model
      ? `${runtimeLabel} · ${formatCodingSessionModelSummary(model)}`
      : (runtimeLabel ??
        (model ? formatCodingSessionModelSummary(model) : null));
  // A seat is both halves. Half a seat — an actor with no role, a role with
  // no actor, or a record projected before either key existed — is not a seat
  // and must label itself exactly as it did before this feature.
  const agentRef = nonEmpty(input.agentRef);
  const role = nonEmpty(input.role);
  if (agentRef === null || role === null) {
    return { primary: runtimeSummary ?? "Coding session", secondary: null };
  }
  const roleLabel = formatCodingSessionRoleLabel(role);
  const name = input.agentDisplayName?.trim();
  return {
    primary: name ? `${name} · ${roleLabel}` : roleLabel,
    secondary: runtimeSummary,
  };
}

/** A role slug as a person reads it: `code-reviewer` → `Code Reviewer`. */
export function formatCodingSessionRoleLabel(role: string): string {
  return role
    .split("-")
    .filter(Boolean)
    .map((token) => `${token.slice(0, 1).toUpperCase()}${token.slice(1)}`)
    .join(" ");
}

function nonEmpty(value: string | null | undefined): string | null {
  return typeof value === "string" && value.trim().length > 0 ? value : null;
}
