import type {
  WaveBMockCommandConfig,
  WaveBMockCommandResult,
} from "./e2eBridgeWaveBRegistry";

/**
 * Mock `coding_session_checkpoint_diff` (SV-30), owned by batch C2 lane D.
 *
 * A spec sets `window.__BEEKEEPER_E2E_CHECKPOINT_DIFF__` in an init script to
 * a map from `"<fromTree>..<toTree>"` to the command's raw answer — one of
 * the four native states, in the Rust command's own wire shape. A pair the
 * map does not name answers `no_checkout`, which is what a computer that
 * recorded no checkout for the session says; with nothing set at all the
 * command passes through to the bridge untouched, so no other spec changes.
 * Paths never appear: the native answer names none.
 */
export type CheckpointDiffMock = Record<string, unknown>;

declare global {
  interface Window {
    __BEEKEEPER_E2E_CHECKPOINT_DIFF__?: CheckpointDiffMock;
  }
}

/** The map key for one tree pair. */
export function checkpointDiffMockKey(
  fromTree: string | null,
  toTree: string,
): string {
  return `${fromTree ?? "null"}..${toTree}`;
}

export async function handleCheckpointDiffMockCommand(
  command: string,
  payload: unknown,
  _config: WaveBMockCommandConfig,
): Promise<WaveBMockCommandResult> {
  if (command !== "coding_session_checkpoint_diff") return null;
  const answers =
    typeof window === "undefined"
      ? undefined
      : window.__BEEKEEPER_E2E_CHECKPOINT_DIFF__;
  if (!answers) return null;
  const args = (payload ?? {}) as { fromTree?: unknown; toTree?: unknown };
  const fromTree = typeof args.fromTree === "string" ? args.fromTree : null;
  const toTree = typeof args.toTree === "string" ? args.toTree : "";
  if (fromTree === null) {
    return { handled: true, value: { state: "baseline_missing" } };
  }
  const answer = answers[checkpointDiffMockKey(fromTree, toTree)];
  return {
    handled: true,
    value: answer ?? { state: "no_checkout" },
  };
}
