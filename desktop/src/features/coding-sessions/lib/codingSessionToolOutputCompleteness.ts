/**
 * Whether a 44225 `tool_result`'s output is known complete.
 *
 * Three optional keys travel together on a result: `contentSource`
 * (`streamed_deltas` | `native_rollout`), `outputComplete`, and — only when
 * the output could not be verified — `outputGap`
 * (`{ streamedBytes, aggregatedBytes? }`). Absent keys mean the content came
 * from the adapter's final frame as it always has; `outputComplete: true`
 * means verified or recovered from the agent's own record. Both render exactly
 * as before.
 *
 * Only `outputComplete === false` is a state worth a notice: the output was
 * assembled from streamed chunks, and codex is known to drop the beginning of
 * a command's output. Nothing here invents that state — a missing or
 * non-boolean `outputComplete` reads as no claim at all — and a malformed
 * `outputGap` costs only the byte counts, never the notice itself.
 */

import type { TranscriptToolOutputGap } from "@/features/agents/ui/agentSessionTypes";
import { isRecord } from "./codingSessionDefensive";

/** The provenance values this client knows; anything else reads as absent. */
export type CodingSessionToolContentSource =
  | "streamed_deltas"
  | "native_rollout";

/** The result's `contentSource`, or `null` when absent or unrecognised. */
export function readCodingSessionToolContentSource(
  value: unknown,
): CodingSessionToolContentSource | null {
  return value === "streamed_deltas" || value === "native_rollout"
    ? value
    : null;
}

function readByteCount(value: unknown): number | undefined {
  return typeof value === "number" &&
    Number.isFinite(value) &&
    Number.isInteger(value) &&
    value >= 0
    ? value
    : undefined;
}

/**
 * The output gap a raw `tool_result` declares, or `null` when its output is
 * complete, recovered, or carries no completeness claim.
 */
export function readCodingSessionToolOutputGap(
  rawResult: unknown,
): TranscriptToolOutputGap | null {
  if (!isRecord(rawResult) || rawResult.outputComplete !== false) return null;
  const gap: TranscriptToolOutputGap = {};
  const declared = isRecord(rawResult.outputGap) ? rawResult.outputGap : {};
  const streamedBytes = readByteCount(declared.streamedBytes);
  const aggregatedBytes = readByteCount(declared.aggregatedBytes);
  if (streamedBytes !== undefined) gap.streamedBytes = streamedBytes;
  // A total smaller than what was captured is self-contradictory; drop it
  // rather than print "captured 900 of 600 bytes".
  if (
    aggregatedBytes !== undefined &&
    (streamedBytes === undefined || aggregatedBytes >= streamedBytes)
  ) {
    gap.aggregatedBytes = aggregatedBytes;
  }
  return gap;
}

/**
 * Spread onto a projected tool item: `{ outputGap }` when the result declares
 * one, `{}` otherwise, so unlabelled results project byte-identically.
 */
export function codingSessionToolOutputGapField(rawResult: unknown): {
  outputGap?: TranscriptToolOutputGap;
} {
  const outputGap = readCodingSessionToolOutputGap(rawResult);
  return outputGap ? { outputGap } : {};
}
