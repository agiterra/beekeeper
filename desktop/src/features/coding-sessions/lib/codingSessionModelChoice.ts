/**
 * Splitting a model id into the choices it actually encodes.
 *
 * Adapters pack several decisions into one string. Codex appends reasoning
 * effort — `gpt-5.4`, `gpt-5.4[low]`, `gpt-5.4[high]`, `gpt-5.4[xhigh]` — and
 * claude-agent-acp appends a context window, `opus[1m]`. Presented raw, that is
 * one control asking three questions at once, and live discovery (§2 item 39)
 * turned seven Codex models into thirty rows.
 *
 * So a bracket is decoded, never guessed at: an enumerated reasoning token
 * becomes the thinking level, a size token (`1m`, `200k`) becomes the context
 * window, and **anything else stays part of the model id**, byte for byte,
 * because inventing a dimension the adapter never offered is worse than a long
 * name.
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
const NO_EXTRA_THINKING: ReadonlySet<string> = new Set();

/** `1m`, `200k`, `128k` — a context window, not a thinking level. */
const CONTEXT_PATTERN = /^\d+(?:\.\d+)?[km]$/i;

/** The alias adapters publish for "you choose" — not a model. */
export const CODING_SESSION_ADAPTER_DEFAULT_MODEL = "default";

/** One published id, split into the decisions it encodes. */
export type CodingSessionModelId = {
  model: string;
  thinking: string | null;
  context: string | null;
};

/**
 * `extraThinking` names effort values a runtime reported for its models
 * (catalog `models[].efforts`, e.g. `ultra`) beyond the enumerated set, so a
 * published `gpt-6-sol[ultra]` decodes as a level rather than a model.
 */
export function splitCodingSessionModelId(
  id: string,
  extraThinking: ReadonlySet<string> = NO_EXTRA_THINKING,
): CodingSessionModelId {
  let rest = id;
  let thinking: string | null = null;
  let context: string | null = null;
  // Right to left, because an adapter may append more than one bracket.
  for (;;) {
    const open = rest.lastIndexOf("[");
    if (open <= 0 || !rest.endsWith("]")) break;
    const tail = rest.slice(open + 1, -1);
    const lowered = tail.toLowerCase();
    if (
      thinking === null &&
      (THINKING.has(lowered) || extraThinking.has(tail))
    ) {
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

/** The inverse: the id an adapter will recognise for this combination. */
export function joinCodingSessionModelId(
  model: string,
  thinking: string | null,
  context: string | null = null,
): string {
  // Context first, then thinking, so `opus[1m]` and `gpt-5.4[high]` both round
  // trip and a model carrying both reads `model[1m][high]`.
  const withContext =
    context === null || context === "" ? model : `${model}[${context}]`;
  return thinking === null || thinking === ""
    ? withContext
    : `${withContext}[${thinking}]`;
}

export type CodingSessionModelChoices = {
  /** Distinct base models, adapter order, with the `default` alias last. */
  models: string[];
  /** Thinking levels offered per base model, adapter order. */
  thinkingByModel: ReadonlyMap<string, string[]>;
  /** Context windows offered per base model, adapter order. */
  contextByModel: ReadonlyMap<string, string[]>;
  /** Base models the adapter also publishes bare, with no bracket at all. */
  bareModels: ReadonlySet<string>;
};

/** Fold an adapter's flat `allowedModels` into the decisions it encodes. */
export function codingSessionModelChoices(
  allowedModels: readonly string[],
  extraThinking: ReadonlySet<string> = NO_EXTRA_THINKING,
): CodingSessionModelChoices {
  const models: string[] = [];
  const thinkingByModel = new Map<string, string[]>();
  const contextByModel = new Map<string, string[]>();
  const bareModels = new Set<string>();
  for (const id of allowedModels) {
    const { model, thinking, context } = splitCodingSessionModelId(
      id,
      extraThinking,
    );
    if (!thinkingByModel.has(model)) {
      models.push(model);
      thinkingByModel.set(model, []);
      contextByModel.set(model, []);
    }
    if (thinking === null && context === null) bareModels.add(model);
    if (thinking !== null) {
      const levels = thinkingByModel.get(model);
      if (levels && !levels.includes(thinking)) levels.push(thinking);
    }
    if (context !== null) {
      const windows = contextByModel.get(model);
      if (windows && !windows.includes(context)) windows.push(context);
    }
  }
  // The `default` alias is not a model and must not sort among them.
  const ordered = [
    ...models.filter((model) => model !== CODING_SESSION_ADAPTER_DEFAULT_MODEL),
    ...models.filter((model) => model === CODING_SESSION_ADAPTER_DEFAULT_MODEL),
  ];
  return { models: ordered, thinkingByModel, contextByModel, bareModels };
}

/**
 * The level to select when the model changes.
 *
 * Carrying the previous choice over is what a person means by "same thinking,
 * different model" — but only when the new model offers it. Otherwise the bare
 * id wins if the adapter has one, and the first offered value if it does not,
 * because a model published only with brackets has no bare form to fall to.
 */
function resolveDimension(
  offered: readonly string[],
  bare: boolean,
  preferred: string | null,
): string | null {
  if (preferred !== null && offered.includes(preferred)) return preferred;
  if (bare) return null;
  return offered[0] ?? null;
}

export function resolveCodingSessionThinking(
  choices: CodingSessionModelChoices,
  model: string,
  preferred: string | null,
): string | null {
  return resolveDimension(
    choices.thinkingByModel.get(model) ?? [],
    choices.bareModels.has(model),
    preferred,
  );
}

export function resolveCodingSessionContext(
  choices: CodingSessionModelChoices,
  model: string,
  preferred: string | null,
): string | null {
  return resolveDimension(
    choices.contextByModel.get(model) ?? [],
    choices.bareModels.has(model),
    preferred,
  );
}
