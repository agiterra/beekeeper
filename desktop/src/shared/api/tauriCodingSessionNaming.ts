import { invokeTauri } from "@/shared/api/tauri";

/**
 * The model that names a coding session from its first message.
 *
 * The key never crosses this boundary. It is written once by
 * {@link setCodingSessionNamingSettings} and from then on the desktop only
 * learns {@link CodingSessionNamingSettings.hasApiKey} — the host holds the
 * secret and builds the request.
 *
 * Mirrors the Rust types in
 * `desktop/src-tauri/src/coding_sessions/naming.rs`.
 */

export type CodingSessionNamingProvider =
  | "off"
  | "anthropic"
  | "openai-compatible";

export type CodingSessionNamingSettings = {
  provider: CodingSessionNamingProvider;
  /** Endpoint root for the OpenAI-compatible adapter. Empty when unused. */
  baseUrl: string;
  model: string;
  /** Whether a key is stored on this computer. Never the key. */
  hasApiKey: boolean;
};

/** Read what names sessions on this computer. */
export async function getCodingSessionNamingSettings(): Promise<CodingSessionNamingSettings> {
  return invokeTauri<CodingSessionNamingSettings>(
    "coding_session_naming_settings",
  );
}

/**
 * Store a new configuration.
 *
 * `apiKey` is three-state: omit it to leave the stored key untouched, pass
 * `""` to delete it, pass a key to replace it.
 */
export async function setCodingSessionNamingSettings(input: {
  provider: CodingSessionNamingProvider;
  baseUrl?: string;
  model?: string;
  apiKey?: string;
}): Promise<CodingSessionNamingSettings> {
  return invokeTauri<CodingSessionNamingSettings>(
    "set_coding_session_naming_settings",
    {
      provider: input.provider,
      baseUrl: input.baseUrl ?? null,
      model: input.model ?? null,
      apiKey: input.apiKey ?? null,
    },
  );
}

/**
 * Ask the configured model for a one-to-four-word name.
 *
 * Rejects — rather than resolving empty — when no model is configured or the
 * endpoint refuses, so the caller can say why no name appeared.
 */
export async function generateCodingSessionName(
  firstMessage: string,
): Promise<string> {
  return invokeTauri<string>("generate_coding_session_name", { firstMessage });
}

/**
 * Ask the configured model for a one-line goal for this first message — the
 * same model and key as the namer. Rejects when no model is configured or
 * the endpoint refuses, so the caller can say why the goal stayed as typed.
 */
export async function generateCodingSessionGoal(
  firstMessage: string,
): Promise<string> {
  return invokeTauri<string>("generate_coding_session_goal", { firstMessage });
}

export type CodingSessionNamingTest = {
  /** The sample that was sent — never anything the person wrote. */
  sent: string;
  name: string;
  /** Round trip in milliseconds. This call repeats every few seconds in the
   * create dialog, so how long it takes is part of whether it is usable. */
  elapsedMs: number;
};

/**
 * Try a configuration without storing it.
 *
 * Everything is optional except the provider: an omitted field falls back to
 * what is stored, which is what makes "test the key I saved last week
 * against the model I just typed" work. An explicitly empty `apiKey` means
 * no key — the right configuration for a local model, and not the same thing
 * as omitting it.
 */
export async function testCodingSessionNaming(input: {
  provider: CodingSessionNamingProvider;
  baseUrl?: string;
  model?: string;
  apiKey?: string;
}): Promise<CodingSessionNamingTest> {
  return invokeTauri<CodingSessionNamingTest>("test_coding_session_naming", {
    provider: input.provider,
    baseUrl: input.baseUrl ?? null,
    model: input.model ?? null,
    apiKey: input.apiKey ?? null,
  });
}
