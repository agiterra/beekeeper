/**
 * Human-facing labels for the provider-neutral runtime/model identifiers a
 * signed session carries.
 *
 * Copied from `desktop/src/features/coding-sessions/lib/codingSessionLabels.ts`
 * plus the model-id splitter it depends on
 * (`codingSessionModelChoice.ts`). Adapters pack several decisions into one
 * model string — Codex appends reasoning effort (`gpt-5.4[high]`),
 * claude-agent-acp appends a context window (`opus[1m]`) — and a bracket is
 * decoded, never guessed at: anything that is not an enumerated reasoning
 * token or a size token stays part of the model id, byte for byte.
 */

/** Reasoning-effort tokens an adapter may append to a model id. */
export const CODING_SESSION_THINKING_LEVELS = [
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
] as const;

const THINKING = new Set<string>(CODING_SESSION_THINKING_LEVELS);

/** `1m`, `200k`, `128k` — a context window, not a thinking level. */
const CONTEXT_PATTERN = /^\d+(?:\.\d+)?[km]$/i;

export type CodingSessionModelId = {
  model: string;
  thinking: string | null;
  context: string | null;
};

export function splitCodingSessionModelId(id: string): CodingSessionModelId {
  let rest = id;
  let thinking: string | null = null;
  let context: string | null = null;
  // Right to left, because an adapter may append more than one bracket.
  for (;;) {
    const open = rest.lastIndexOf("[");
    if (open <= 0 || !rest.endsWith("]")) break;
    const tail = rest.slice(open + 1, -1);
    const lowered = tail.toLowerCase();
    if (thinking === null && THINKING.has(lowered)) {
      thinking = tail;
    } else if (context === null && CONTEXT_PATTERN.test(tail)) {
      context = tail;
    } else {
      break;
    }
    rest = rest.slice(0, open);
  }
  return { model: rest, thinking, context };
}

export function formatCodingSessionRuntimeLabel(runtime: string): string {
  const normalized = runtime.trim().toLowerCase().split("_").join("-");
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

/** A compact label for places that cannot render the dimensions separately. */
export function formatCodingSessionModelSummary(modelId: string): string {
  const parsed = splitCodingSessionModelId(modelId);
  return [
    parsed.model,
    parsed.thinking ? titleCaseLabel(parsed.thinking) : null,
    parsed.context ? parsed.context.toUpperCase() : null,
  ]
    .filter((value): value is string => Boolean(value))
    .join(" · ");
}

/** The execution chip: `agentRef` when the provider bound one, else runtime · model. */
export function formatCodingSessionExecutionLabel(input: {
  runtime: string | null;
  model: string | null;
  agentRef: string | null;
  driver: string | null;
}): string {
  if (input.agentRef !== null && input.agentRef.trim().length > 0) {
    return input.agentRef;
  }
  const runtimeLabel = formatCodingSessionRuntimeLabel(
    input.runtime ?? input.driver ?? "coding session",
  );
  return input.model
    ? `${runtimeLabel} · ${formatCodingSessionModelSummary(input.model)}`
    : runtimeLabel;
}

function titleCaseLabel(value: string): string {
  return `${value.slice(0, 1).toUpperCase()}${value.slice(1).toLowerCase()}`;
}
