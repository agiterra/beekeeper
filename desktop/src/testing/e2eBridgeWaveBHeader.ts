import type {
  WaveBMockCommandConfig,
  WaveBMockCommandResult,
} from "./e2eBridgeWaveBRegistry";

/**
 * Mock Tauri commands for the session header (SV-20), owned by lane B1.
 *
 * Tried before the bridge's built-in `switch` (`e2eBridge.ts`). Returns
 * `{ handled: true, value }` to answer a command, or `null` to pass it on.
 *
 * It answers one command, and only for a spec that asks it to: when the
 * spec's mock config carries `waveBHeader.observationFold`, the session's
 * kind-44246 fold (`fold_coding_session_observations_command`) answers with
 * that response. The header spec seeds a failed observed gate row this way,
 * so the Landing surface's own badge — not the header — decides the tone the
 * right-panel toggle's dot shows. Every other spec passes straight through.
 */
const OBSERVATION_FOLD_COMMAND = "fold_coding_session_observations_command";

function waveBHeaderConfig(
  config: WaveBMockCommandConfig,
): { observationFold?: unknown } | null {
  const value = config?.mock?.waveBHeader;
  return typeof value === "object" && value !== null
    ? (value as { observationFold?: unknown })
    : null;
}

export async function handleWaveBHeaderMockCommand(
  command: string,
  _payload: unknown,
  config: WaveBMockCommandConfig,
): Promise<WaveBMockCommandResult> {
  if (command !== OBSERVATION_FOLD_COMMAND) return null;
  const fold = waveBHeaderConfig(config)?.observationFold;
  if (fold === undefined || fold === null) return null;
  return { handled: true, value: structuredClone(fold) };
}
