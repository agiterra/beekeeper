import { Terminal } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import { ScrollFadeMonoPanel } from "../FileContentBlock";
import type { TranscriptToolOutputGap } from "../agentSessionTypes";
import {
  parseShellToolOutput,
  type ShellToolOutput,
} from "../agentSessionUtils";
import { ToolOutputGapNotice } from "./ToolOutputGapNotice";

/**
 * The text shown under a shell command.
 *
 * On success only stdout matters — stderr is routinely noisy on a command that
 * worked. On a failure the opposite is true: the reason a call failed is almost
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
    return output.stdout.trimEnd();
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
          {command}
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
            {shellOutput}
          </pre>
        </ScrollFadeMonoPanel>
      ) : null}
    </div>
  );
}
