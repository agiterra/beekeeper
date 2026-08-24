/**
 * Splitting a model id into the two choices it actually encodes.
 *
 * Codex advertises its reasoning effort as part of the model id —
 * `gpt-5.4`, `gpt-5.4[low]`, `gpt-5.4[medium]`, `gpt-5.4[high]`,
 * `gpt-5.4[xhigh]` — so live discovery (§2 item 39) turned one four-model
 * adapter into a thirty-row dropdown where the same model appeared five times.
 * The list is honest; the shape is not, because it presents one control for
 * two independent decisions.
 *
 * Not every bracket is an effort. `claude-agent-acp` offers `opus[1m]`, where
 * the tail is a context window, and treating it as a thinking level would
 * invent a choice the adapter never offered. Only the enumerated tokens below
 * split; everything else stays part of the model id, byte for byte.
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

export type CodingSessionThinkingLevel =
  (typeof CODING_SESSION_THINKING_LEVELS)[number];

const THINKING = new Set<string>(CODING_SESSION_THINKING_LEVELS);

/** One model id, split into the model and the thinking level it encodes. */
export function splitCodingSessionModelId(id: string): {
  model: string;
  thinking: string | null;
} {
  const open = id.lastIndexOf("[");
  if (open <= 0 || !id.endsWith("]")) return { model: id, thinking: null };
  const tail = id.slice(open + 1, -1);
  if (!THINKING.has(tail.toLowerCase())) return { model: id, thinking: null };
  return { model: id.slice(0, open), thinking: tail };
}

/** The inverse: the id an adapter will recognise for this pair. */
export function joinCodingSessionModelId(
  model: string,
  thinking: string | null,
): string {
  return thinking === null || thinking === "" ? model : `${model}[${thinking}]`;
}

export type CodingSessionModelChoices = {
  /** Distinct base models, in the order the adapter listed them. */
  models: string[];
  /**
   * Thinking levels offered per base model, adapter order. Empty means the
   * adapter offers no levels for it and the second control has nothing to say.
   */
  thinkingByModel: ReadonlyMap<string, string[]>;
  /**
   * Base models the adapter also offers with no level at all. Selecting one of
   * those keeps the bare id, which is not the same request as picking a level
   * — some adapters resolve the bare id to their own default.
   */
  bareModels: ReadonlySet<string>;
};

/** Fold an adapter's flat `allowedModels` into the two choices it encodes. */
export function codingSessionModelChoices(
  allowedModels: readonly string[],
): CodingSessionModelChoices {
  const models: string[] = [];
  const thinkingByModel = new Map<string, string[]>();
  const bareModels = new Set<string>();
  for (const id of allowedModels) {
    const { model, thinking } = splitCodingSessionModelId(id);
    if (!thinkingByModel.has(model)) {
      models.push(model);
      thinkingByModel.set(model, []);
    }
    if (thinking === null) {
      bareModels.add(model);
      continue;
    }
    const levels = thinkingByModel.get(model);
    if (levels && !levels.includes(thinking)) levels.push(thinking);
  }
  return { models, thinkingByModel, bareModels };
}

/**
 * The level to select when the model changes.
 *
 * Carrying the previous level over is what a person means by "same thinking,
 * different model" — but only when the new model actually offers it. Otherwise
 * the bare id wins if the adapter has one, and the first offered level if it
 * does not, because a model with only levelled ids has no default to fall to.
 */
export function resolveCodingSessionThinking(
  choices: CodingSessionModelChoices,
  model: string,
  preferred: string | null,
): string | null {
  const levels = choices.thinkingByModel.get(model) ?? [];
  if (preferred !== null && levels.includes(preferred)) return preferred;
  if (choices.bareModels.has(model)) return null;
  return levels[0] ?? null;
}
