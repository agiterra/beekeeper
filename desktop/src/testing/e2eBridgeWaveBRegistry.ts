import { handleWaveBBadgesMockCommand } from "./e2eBridgeWaveBBadges";
import { handleWaveBHeaderMockCommand } from "./e2eBridgeWaveBHeader";
import { handleWaveBMinimapMockCommand } from "./e2eBridgeWaveBMinimap";
import { handleWaveBOrchestrationMockCommand } from "./e2eBridgeWaveBOrchestration";
import { handleWaveBSurfacesMockCommand } from "./e2eBridgeWaveBSurfaces";
import { handleWaveBTerminalMockCommand } from "./e2eBridgeWaveBTerminal";

/**
 * The session-view parity Wave B seam in the E2E bridge (brief §4).
 *
 * `e2eBridge.ts` calls {@link handleWaveBMockCommand} before its built-in
 * `switch`; this tries each lane's own module in turn, so a phase-2 lane mocks
 * its commands in its own file and never edits the shared bridge.
 */

/** The active spec's config, loosely typed: each lane reads its own keys. */
export type WaveBMockCommandConfig =
  | { mock?: Record<string, unknown> | undefined }
  | null
  | undefined;

/** `null` passes the command on; otherwise the value the command returns. */
export type WaveBMockCommandResult = { handled: true; value: unknown } | null;

type WaveBHandler = (
  command: string,
  payload: unknown,
  config: WaveBMockCommandConfig,
) => Promise<WaveBMockCommandResult>;

/** B0's own commands: none yet. The tree commands stay unmocked by default. */
async function handleWaveBRegistryMockCommand(
  _command: string,
  _payload: unknown,
  _config: WaveBMockCommandConfig,
): Promise<WaveBMockCommandResult> {
  return null;
}

const HANDLERS: readonly WaveBHandler[] = [
  handleWaveBRegistryMockCommand,
  handleWaveBHeaderMockCommand,
  handleWaveBSurfacesMockCommand,
  handleWaveBBadgesMockCommand,
  handleWaveBTerminalMockCommand,
  handleWaveBMinimapMockCommand,
  handleWaveBOrchestrationMockCommand,
];

/** Ask each Wave B lane module in turn; the first to answer wins. */
export async function handleWaveBMockCommand(
  command: string,
  payload: unknown,
  config: WaveBMockCommandConfig,
): Promise<WaveBMockCommandResult> {
  for (const handler of HANDLERS) {
    const result = await handler(command, payload, config);
    if (result !== null) return result;
  }
  return null;
}
