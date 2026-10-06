import type {
  WaveBMockCommandConfig,
  WaveBMockCommandResult,
} from "./e2eBridgeWaveBRegistry";

/** The project-work fold command (`shared/api/tauriProjectWork.ts`). */
const PROJECT_WORK_COVERAGE_COMMAND = "project_work_coverage";

/**
 * The spec key carrying a canned kind-44249 coverage answer. A spec sets it
 * under `mock` to stand in for the native fold; absent, the command passes on
 * to the bridge's own answer.
 */
export const WAVE_B_ORCHESTRATION_PROJECT_WORK_KEY =
  "waveBOrchestrationProjectWork";

/**
 * Mock Tauri commands for the Agents orchestration view (SV-40), owned by lane B6.
 *
 * Tried before the bridge's built-in `switch` (`e2eBridge.ts`). Return
 * `{ handled: true, value }` to answer a command, or `null` to pass it on.
 *
 * The orchestration view reads relay events through the surface context
 * (44223 metadata, 44225 transcripts) and starts no command of its own, so
 * the only command it can observe is the 44249 work-coverage fold the
 * Mission surface runs; a spec that seeds declared work answers it here.
 * Kind 44200 needs no mock: it is NIP-44 encrypted to the agent's owner,
 * carries no channel tag, and the coding-session provider never publishes it
 * (only the `buzz-acp` harness does, `crates/beekeeper-acp/src/pool.rs`), so this
 * view reads tokens from the signed 44225 turn results instead.
 */
export async function handleWaveBOrchestrationMockCommand(
  command: string,
  _payload: unknown,
  config: WaveBMockCommandConfig,
): Promise<WaveBMockCommandResult> {
  if (command !== PROJECT_WORK_COVERAGE_COMMAND) return null;
  const canned = config?.mock?.[WAVE_B_ORCHESTRATION_PROJECT_WORK_KEY];
  return canned === undefined ? null : { handled: true, value: canned };
}
