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

/**
 * Who titles a new coding session on this computer (D9, SV-56).
 *
 * - `agent`: the provider that runs the first turn titles it with the same
 *   runtime and account that already received the message (SV-31). The
 *   default, and nothing to set up.
 * - `my-model`: the person's own naming model below suggests a name in the
 *   Name field while they write. Nothing is titled after Start.
 * - `off`: nothing is asked for a title anywhere.
 *
 * A name a person typed wins in every mode. This is this computer's
 * preference: it reaches this machine's agent host as a local file, never
 * the relay.
 */
export type CodingSessionTitleMode = "agent" | "my-model" | "off";

export const CODING_SESSION_TITLE_MODES: readonly CodingSessionTitleMode[] = [
  "agent",
  "my-model",
  "off",
];

export type CodingSessionNamingSettings = {
  /**
   * The mode in force. A record saved before modes existed reads as
   * `my-model` when it had a naming endpoint configured (that was an opt-in)
   * and `agent` otherwise.
   */
  titleMode: CodingSessionTitleMode;
  provider: CodingSessionNamingProvider;
  /** Endpoint root for the OpenAI-compatible adapter. Empty when unused. */
  baseUrl: string;
  model: string;
  /** Whether a key is stored on this computer. Never the key. */
  hasApiKey: boolean;
  /**
   * Set when this computer's agent-host files do not hold `titleMode`: a
   * save or a provisioning could not write them, or one cannot be read.
   * The host re-reads the files on every settings read, so this stays set
   * until a later write lands. Optional only so drafts built in the webview
   * need not carry it; the host always sends it (`null` when in step).
   */
  hostModeMismatch?: string | null;
};

/** Read what names sessions on this computer. */
export async function getCodingSessionNamingSettings(): Promise<CodingSessionNamingSettings> {
  return invokeTauri<CodingSessionNamingSettings>(
    "coding_session_naming_settings",
  );
}

/**
 * Whether the founded flow may ask the person's naming model at all.
 *
 * Only in `my-model` mode, and only with an endpoint chosen. The host
 * refuses `generate_coding_session_name` / `generate_coding_session_goal`
 * in every other mode, so this is the webview's courtesy, not the fence.
 */
export function namingModelConsulted(
  settings: CodingSessionNamingSettings | null,
): boolean {
  return (
    settings !== null &&
    settings.titleMode === "my-model" &&
    settings.provider !== "off"
  );
}

/**
 * Store a new configuration.
 *
 * `apiKey` is three-state: omit it to leave the stored key untouched, pass
 * `""` to delete it, pass a key to replace it. `titleMode` omitted leaves
 * the mode in force unchanged. Saving also writes the mode to every agent
 * host identity on this computer. A failure there resolves — the record and
 * key already changed — with `hostModeMismatch` saying the host was not
 * told. A rejection means the record was not saved, though a key change may
 * already have reached the keychain, so callers re-read after any error.
 */
export async function setCodingSessionNamingSettings(input: {
  provider: CodingSessionNamingProvider;
  titleMode?: CodingSessionTitleMode;
  baseUrl?: string;
  model?: string;
  apiKey?: string;
}): Promise<CodingSessionNamingSettings> {
  return invokeTauri<CodingSessionNamingSettings>(
    "set_coding_session_naming_settings",
    {
      provider: input.provider,
      titleMode: input.titleMode ?? null,
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
