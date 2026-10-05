import type {
  CodingSessionNamingProvider,
  CodingSessionNamingSettings,
  CodingSessionTitleMode,
} from "@/shared/api/tauriCodingSessionNaming";

/**
 * The words the Session titles card uses for each mode (D9, SV-56).
 *
 * Each mode's disclosure answers the two questions a person is owed before
 * choosing: where does the first message go for a title, and on which
 * machine. The agent mode's answer depends on the computer that *runs* the
 * session, not the one showing this card, and a host can be switched off by
 * an environment variable no card can see — both are said, never implied.
 */

export const CODING_SESSION_TITLE_MODE_LABELS: Record<
  CodingSessionTitleMode,
  string
> = {
  agent: "Generate with the session's agent",
  "my-model": "Use my naming model",
  off: "Off",
};

/** One line under each choice, short enough to scan. */
export const CODING_SESSION_TITLE_MODE_HINTS: Record<
  CodingSessionTitleMode,
  string
> = {
  agent: "Default. Nothing to set up.",
  "my-model": "Your own endpoint suggests a name while you write.",
  off: "Sessions stay untitled until someone names them.",
};

/** Said once, above the choice: it holds in every mode. */
export const CODING_SESSION_TYPED_NAME_WINS =
  "A name someone types always wins, in every mode.";

export const CODING_SESSION_AGENT_MODE_DISCLOSURE =
  "The agent that runs the first turn titles the session on the computer it runs on, with the same account that already received the message; nothing new leaves this computer. Applies to sessions run by this computer's agent host; a session on another computer follows that computer's setting. A host started with BUZZ_CSP_AUTO_TITLE=off, or a runtime configured with no title model, titles nothing.";

export const CODING_SESSION_MY_MODEL_AFTER_START =
  "Your model suggests a name in the Name field while you write; nothing is titled after Start.";

export const CODING_SESSION_OFF_MODE_DISCLOSURE =
  "Nothing is sent anywhere for a title; sessions stay untitled until someone names them.";

/**
 * What the chosen naming endpoint receives, in one sentence.
 *
 * The endpoint is named, because "a model" is not an answer to where a
 * person's words went.
 */
export function namerDisclosure(
  provider: CodingSessionNamingProvider,
  baseUrl: string,
): string {
  if (provider === "off") {
    return "No endpoint is chosen yet, so nothing is sent. Choose one below to get suggestions.";
  }
  if (provider === "anthropic") {
    return "Your first message is sent to api.anthropic.com every few seconds while you write it, and when you leave the field.";
  }
  const target =
    baseUrl.trim().length > 0 ? baseUrl.trim() : "the API URL below";
  return `Your first message is sent to ${target} every few seconds while you write it, and when you leave the field. A local address keeps it on this computer.`;
}

/**
 * The full disclosure for one mode, as the paragraphs the card shows.
 *
 * `my-model` also says what the host does in that mode — titles nothing —
 * because a person choosing their own model should not be surprised that
 * the agent stopped titling sessions on this computer.
 */
export function codingSessionTitleModeDisclosure(input: {
  mode: CodingSessionTitleMode;
  provider: CodingSessionNamingProvider;
  baseUrl: string;
}): string[] {
  switch (input.mode) {
    case "agent":
      return [CODING_SESSION_AGENT_MODE_DISCLOSURE];
    case "my-model":
      return [
        namerDisclosure(input.provider, input.baseUrl),
        CODING_SESSION_MY_MODEL_AFTER_START,
        "This computer's agent host titles nothing in this mode; a session on another computer follows that computer's setting.",
      ];
    case "off":
      return [
        CODING_SESSION_OFF_MODE_DISCLOSURE,
        "Applies to this computer's agent host and Name field; a session on another computer follows that computer's setting.",
      ];
  }
}

/** The mode a fresh computer is on, and the one "Reset to default" restores. */
export const CODING_SESSION_TITLE_MODE_DEFAULT: CodingSessionTitleMode =
  "agent";

/**
 * Said while the choice on screen is not the one stored — "Use my naming
 * model" with no endpoint saved yet — so the card never reads as in force
 * before Save.
 */
export function codingSessionTitleModeUnsaved(
  stored: CodingSessionTitleMode,
  chosen: CodingSessionTitleMode,
): string | null {
  if (stored === chosen) return null;
  return `Not saved — this computer is still on “${CODING_SESSION_TITLE_MODE_LABELS[stored]}”.`;
}

/**
 * The draft after choosing `mode`.
 *
 * Choosing "Use my naming model" with no endpoint picks the
 * OpenAI-compatible adapter, so the first fields shown are the ones that can
 * keep the message on this computer (a local URL); nothing is sent until a
 * URL and model are saved.
 */
export function chooseCodingSessionTitleMode(
  draft: CodingSessionNamingSettings,
  mode: CodingSessionTitleMode,
): CodingSessionNamingSettings {
  if (mode === "my-model" && draft.provider === "off") {
    return { ...draft, titleMode: mode, provider: "openai-compatible" };
  }
  return { ...draft, titleMode: mode };
}

/**
 * What Save sends.
 *
 * The endpoint fields are only on screen in "Use my naming model", so only
 * that mode saves them; the other two leave the stored endpoint exactly as
 * it was, ready for the day the person switches back. The key is omitted
 * unless typed — an empty string would delete it.
 */
export function codingSessionTitleModeSaveInput(input: {
  draft: CodingSessionNamingSettings;
  stored: CodingSessionNamingSettings | null;
  apiKey: string;
}): {
  titleMode: CodingSessionTitleMode;
  provider: CodingSessionNamingProvider;
  baseUrl?: string;
  model?: string;
  apiKey?: string;
} {
  const { draft, stored, apiKey } = input;
  if (draft.titleMode !== "my-model") {
    return {
      titleMode: draft.titleMode,
      provider: stored?.provider ?? draft.provider,
    };
  }
  return {
    titleMode: draft.titleMode,
    provider: draft.provider,
    baseUrl: draft.baseUrl,
    model: draft.model,
    ...(apiKey.length > 0 ? { apiKey } : {}),
  };
}

/** What choosing a mode, or resetting it, sends at once. */
export type CodingSessionTitleModeApply = {
  titleMode: CodingSessionTitleMode;
  provider: CodingSessionNamingProvider;
};

function endpointUnchanged(
  draft: CodingSessionNamingSettings,
  stored: CodingSessionNamingSettings,
): boolean {
  return (
    draft.provider === stored.provider &&
    draft.baseUrl === stored.baseUrl &&
    draft.model === stored.model
  );
}

/**
 * Whether choosing `mode` applies at once, and what it sends (T3's
 * settings apply on change; a three-way choice carries no secret).
 *
 * "Agent" and "Off" always apply, with the stored endpoint left as it is.
 * "Use my naming model" applies only when an endpoint is already stored and
 * the fields on screen still say it — otherwise there is an endpoint to
 * fill in first, and that takes Save (`null`).
 */
export function codingSessionTitleModeApplyOnChoose(input: {
  stored: CodingSessionNamingSettings | null;
  draft: CodingSessionNamingSettings;
  mode: CodingSessionTitleMode;
}): CodingSessionTitleModeApply | null {
  const { stored, draft, mode } = input;
  if (stored === null) return null;
  if (
    mode === "my-model" &&
    (stored.provider === "off" || !endpointUnchanged(draft, stored))
  ) {
    return null;
  }
  return { titleMode: mode, provider: stored.provider };
}

/** "Reset to default" is offered only while the stored mode is not it. */
export function codingSessionTitleModeReset(
  stored: CodingSessionNamingSettings | null,
): CodingSessionTitleModeApply | null {
  if (stored === null || stored.titleMode === CODING_SESSION_TITLE_MODE_DEFAULT)
    return null;
  return {
    titleMode: CODING_SESSION_TITLE_MODE_DEFAULT,
    provider: stored.provider,
  };
}

/**
 * The draft after a save, a mode change or "Forget key" failed and the
 * stored settings were re-read.
 *
 * A failure can come after the keychain changed, so the key state always
 * comes from what is stored. A failed mode change also puts the radio back
 * on the stored mode. A failed endpoint Save keeps the person's fields —
 * they are what is being fixed — and the unsaved line says the rest.
 */
export function codingSessionNamingDraftAfterError(
  draft: CodingSessionNamingSettings | null,
  fresh: CodingSessionNamingSettings,
  revertMode: boolean,
): CodingSessionNamingSettings {
  if (draft === null) return fresh;
  return {
    ...draft,
    hasApiKey: fresh.hasApiKey,
    hostModeMismatch: fresh.hostModeMismatch ?? null,
    ...(revertMode ? { titleMode: fresh.titleMode } : {}),
  };
}

/**
 * The standing warning while this computer's agent host files do not hold
 * the stored mode. `null` when they do.
 */
export function codingSessionHostMismatchLine(
  stored: CodingSessionNamingSettings | null,
): string | null {
  if (stored === null || !stored.hostModeMismatch) return null;
  return `“${CODING_SESSION_TITLE_MODE_LABELS[stored.titleMode]}” is saved on this computer, but ${stored.hostModeMismatch}. Until it is told, that host may title sessions differently from what this card says.`;
}
