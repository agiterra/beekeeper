import type { CodingSessionSubagentPrompt } from "@/features/coding-sessions/lib/codingSessionSubagentPageModel";

/**
 * The prompt the subagent was given (SV-82), as the first block of its page.
 * A prompt the provider bounded on the way out, or one the call never
 * carried, is disclosed as such rather than left out.
 */
export function CodingSessionSubagentPromptBlock({
  prompt,
}: {
  prompt: CodingSessionSubagentPrompt;
}) {
  return (
    <section
      className="flex flex-col gap-1.5 rounded-md border border-border/60 bg-muted/20 px-3 py-2"
      data-prompt={prompt.kind}
      data-testid="coding-session-subagent-page-prompt"
    >
      <p className="text-2xs font-semibold tracking-wide text-muted-foreground uppercase">
        Prompt it was given
      </p>
      {prompt.kind === "given" ? (
        <p className="text-sm whitespace-pre-wrap break-words">{prompt.text}</p>
      ) : prompt.kind === "truncated" ? (
        <>
          <p className="text-sm text-muted-foreground">
            The call's input was too large to publish in full
            {prompt.byteCount !== null
              ? ` (${prompt.byteCount.toLocaleString()} bytes)`
              : ""}
            ; only its beginning survives, as published:
          </p>
          {prompt.preview ? (
            <pre className="max-h-64 overflow-auto rounded-sm bg-muted/40 p-2 font-mono text-xs whitespace-pre-wrap break-words">
              {prompt.preview}
            </pre>
          ) : null}
        </>
      ) : (
        <p className="text-sm text-muted-foreground">
          The prompt this subagent was given is not in the transcript.
        </p>
      )}
    </section>
  );
}
