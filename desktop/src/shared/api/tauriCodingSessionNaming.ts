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
