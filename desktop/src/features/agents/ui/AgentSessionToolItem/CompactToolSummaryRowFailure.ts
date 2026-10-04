import { parseShellToolOutput } from "../agentSessionUtils";

/**
 * How a failed tool call's collapsed row reads (SV-02, decision D2).
 *
 * - `alarm` — the destructive "Tool call failed" row. For a failure the agent
 *   did not recover from: one after the turn's answer, or in a turn that
 *   never answered. This is the default, so any caller that does not know
 *   where the row sits keeps the loud presentation.
 * - `quiet` — a failed step inside a settled turn's "Worked for …" fold: a
 *   dimmed red icon, the call in ordinary row text, and the word "failed" (or
 *   the exit code) after it. Muted, never unmarked: the icon carries an
 *   accessible "Failed" name and the suffix stays on screen.
 */
export type CompactToolFailureTone = "alarm" | "quiet";

/**
 * Claude's Bash reports a failed command as plain text, not a JSON shell
 * result: `Exit code 2\n<output>`, sometimes behind an `Error: ` prefix. Only
 * a leading line counts — an "exit code" quoted later in the output is the
 * command's own text, not its status.
 */
const PLAIN_TEXT_EXIT_CODE = /^\s*(?:Error:\s*)?Exit code (\d+)/;

/**
 * The short suffix a quiet failed row ends with: `exit 2` when the result is
 * a shell result naming its exit code (a JSON shell result, or Claude's plain
 * `Exit code 2` text), `timed out` when it says so, and `failed` otherwise.
 * Never empty, so a quiet row always says it failed.
 */
export function describeCompactToolFailure(result: string): string {
  const output = parseShellToolOutput(result);
  if (output.timedOut) return "timed out";
  if (output.exitCode !== null && output.exitCode !== 0) {
    return `exit ${output.exitCode}`;
  }
  if (output.exitCode === null) {
    const match = PLAIN_TEXT_EXIT_CODE.exec(output.raw);
    const code = match ? Number(match[1]) : 0;
    if (code !== 0) return `exit ${code}`;
  }
  return "failed";
}
