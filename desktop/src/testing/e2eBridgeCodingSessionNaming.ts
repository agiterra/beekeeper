/**
 * Mock of the session-title settings commands (`coding_session_naming_settings`,
 * `set_coding_session_naming_settings`; `desktop/src-tauri/src/coding_sessions/naming.rs`),
 * answered only when a spec seeds `mock.codingSessionNaming` (D9, SV-56).
 *
 * Unseeded, both commands stay unsupported exactly as before — the card then
 * says it could not read the setting — so no other spec changes. Seeded, the
 * mock keeps the stored settings on `window` and records every save, so a
 * spec can assert what the card actually sent.
 */

export type MockCodingSessionTitleMode = "agent" | "my-model" | "off";

export type MockCodingSessionNamingSettings = {
  titleMode: MockCodingSessionTitleMode;
  provider: "off" | "anthropic" | "openai-compatible";
  baseUrl: string;
  model: string;
  hasApiKey: boolean;
  /**
   * The host's "agent host was not told" sentence. A spec seeds it to show
   * the standing warning; any save clears it, as a write that lands does.
   */
  hostModeMismatch?: string | null;
};

/** One recorded `set_coding_session_naming_settings` payload, as sent. */
export type MockCodingSessionNamingSetCall = Record<string, unknown>;

declare global {
  interface Window {
    __BUZZ_E2E_CODING_SESSION_NAMING__?: MockCodingSessionNamingSettings;
    __BUZZ_E2E_CODING_SESSION_NAMING_SET_CALLS__?: MockCodingSessionNamingSetCall[];
  }
}

const COMMANDS = new Set([
  "coding_session_naming_settings",
  "set_coding_session_naming_settings",
]);

/** The host's legacy rule: an endpoint configured meant "my model". */
function effectiveMode(
  stored: MockCodingSessionTitleMode | null | undefined,
  provider: MockCodingSessionNamingSettings["provider"],
): MockCodingSessionTitleMode {
  if (stored) return stored;
  return provider === "off" ? "agent" : "my-model";
}

/**
 * Apply one save to `current` the way the host does: omitted fields stay,
 * the mode is fixed before the endpoint changes, `my-model` needs an
 * endpoint, an empty key deletes the stored one, and the agent host is
 * told (so any standing mismatch clears).
 */
export function applyMockCodingSessionNamingSave(
  current: MockCodingSessionNamingSettings,
  payload: Record<string, unknown>,
): MockCodingSessionNamingSettings {
  const provider =
    (payload.provider as MockCodingSessionNamingSettings["provider"]) ??
    current.provider;
  const titleMode = effectiveMode(
    (payload.titleMode as MockCodingSessionTitleMode | null) ??
      current.titleMode,
    provider,
  );
  if (titleMode === "my-model" && provider === "off") {
    throw new Error(
      "“Use my naming model” needs an endpoint: choose the Anthropic API or an OpenAI-compatible API.",
    );
  }
  const apiKey = payload.apiKey as string | null | undefined;
  return {
    titleMode,
    provider,
    baseUrl:
      typeof payload.baseUrl === "string" ? payload.baseUrl : current.baseUrl,
    model: typeof payload.model === "string" ? payload.model : current.model,
    hasApiKey:
      typeof apiKey === "string" ? apiKey.trim().length > 0 : current.hasApiKey,
    hostModeMismatch: null,
  };
}

/**
 * Route one of the two commands; returns `undefined` for any other command
 * and throws the bridge's usual "unsupported" error when unseeded.
 */
export function handleMockCodingSessionNamingCommand(
  command: string,
  payload: unknown,
  seed: MockCodingSessionNamingSettings | undefined,
): unknown | undefined {
  if (!COMMANDS.has(command)) return undefined;
  if (!seed) throw new Error(`Unsupported mocked Tauri command: ${command}`);
  window.__BUZZ_E2E_CODING_SESSION_NAMING__ ??= { ...seed };
  const current = window.__BUZZ_E2E_CODING_SESSION_NAMING__;
  if (command === "coding_session_naming_settings") {
    return { hostModeMismatch: null, ...current };
  }
  const request = { ...((payload ?? {}) as Record<string, unknown>) };
  window.__BUZZ_E2E_CODING_SESSION_NAMING_SET_CALLS__ ??= [];
  window.__BUZZ_E2E_CODING_SESSION_NAMING_SET_CALLS__.push(request);
  const next = applyMockCodingSessionNamingSave(current, request);
  window.__BUZZ_E2E_CODING_SESSION_NAMING__ = next;
  return { ...next };
}
