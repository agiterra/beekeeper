import { Terminal } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import { RedactedText } from "@/shared/ui/RedactedPill";
import { ScrollFadeMonoPanel } from "../FileContentBlock";
import type { TranscriptToolOutputGap } from "../agentSessionTypes";
import {
  parseShellToolOutput,
  type ShellToolOutput,
  stripWholeMarkdownFence,
} from "../agentSessionUtils";
import { ToolOutputGapNotice } from "./ToolOutputGapNotice";

/**
 * The text shown under a shell command.
 *
 * On success only stdout matters — stderr is routinely noisy on a command that
 * worked. A runtime that returns plain text instead of a shell envelope
 * (claude-agent-acp wraps it in a code fence) leaves stdout empty and the
 * output in `raw`; on success that text, unfenced, is the output (SV-92). On a failure the opposite is true: the reason a call failed is almost
 * always in stderr or the exit code, and showing stdout alone renders an empty
 * panel under a red "failed" row. So a failed call shows everything the parser
 * recovered, exit code first.
 *
 * Exported for testing; the component below is the only caller.
 */
export function shellBlockOutput(
  output: ShellToolOutput,
  isError: boolean,
): string {
  if (!isError) {
    const stdout = output.stdout.trimEnd();
    return stdout || stripWholeMarkdownFence(output.raw).trimEnd();
  }
  return [
    output.exitCode !== null ? `Exit code ${output.exitCode}` : "",
    output.stdout,
    output.stderr,
    output.raw,
  ]
    .filter(Boolean)
    .join("\n")
    .trimEnd();
}

/**
 * Does this command print only paths, one per line (`pwd`, `realpath x`,
 * `git rev-parse --show-toplevel`)? Then a redaction marker alone on an
 * output line stood for a path, and its chip can say so.
 *
 * Exported for testing.
 */
export function shellOutputIsPathPerLine(command: string): boolean {
  return /^\s*(?:pwd|realpath|readlink(?:\s+-f)?|dirname|git\s+rev-parse\s+--show-toplevel)(?:\s[^|;&]*)?\s*$/.test(
    command,
  );
}

export function ShellCommandBlock({
  command,
  isError,
  outputGap,
  result,
}: {
  command: string;
  isError: boolean;
  /** Set only when the output may be missing its beginning. */
  outputGap?: TranscriptToolOutputGap;
  result: string;
}) {
  const shellOutput = shellBlockOutput(parseShellToolOutput(result), isError);
  const fadeFromClassName = isError ? "from-destructive/10" : "from-muted";

  return (
    <div
      className={cn(
        "overflow-hidden rounded-lg font-mono text-xs leading-5",
        isError ? "bg-destructive/10" : "bg-muted",
      )}
      data-testid="transcript-shell-command"
    >
      <ScrollFadeMonoPanel
        fadeFromClassName={fadeFromClassName}
        maxHeightClassName="max-h-36"
      >
        <p className="whitespace-pre-wrap wrap-break-word text-muted-foreground/70">
          <Terminal className="mr-2 inline h-3.5 w-3.5 align-[-0.1875rem] text-primary" />
          <RedactedText text={command} />
        </p>
      </ScrollFadeMonoPanel>
      {outputGap ? (
        <div className="mt-2 font-sans">
          <ToolOutputGapNotice gap={outputGap} />
        </div>
      ) : null}
      {shellOutput ? (
        <ScrollFadeMonoPanel
          className="mt-2"
          fadeFromClassName={fadeFromClassName}
          maxHeightClassName="max-h-36"
        >
          <pre
            className={cn(
              "whitespace-pre-wrap wrap-break-word",
              isError ? "text-destructive" : "text-foreground",
            )}
          >
            <RedactedText
              text={shellOutput}
              wholeLinesArePaths={shellOutputIsPathPerLine(command)}
            />
          </pre>
        </ScrollFadeMonoPanel>
      ) : null}
    </div>
  );
}
