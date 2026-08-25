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
