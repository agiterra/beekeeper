import type { ProjectTeamSetupFailure } from "../lib/projectTeamSetup";

/** A plain-language failure with the raw host message kept under Details. */
export function ProjectTeamSetupFailureNotice({
  failure,
}: {
  failure: ProjectTeamSetupFailure | null;
}) {
  if (!failure) return null;
  return (
    <div className="space-y-1 text-sm text-destructive" role="alert">
      <p>{failure.summary}</p>
      {failure.detail ? (
        <details className="text-muted-foreground">
          <summary className="cursor-pointer">Details</summary>
          <p className="break-words">{failure.detail}</p>
          {failure.code ? <p className="font-mono">{failure.code}</p> : null}
        </details>
      ) : null}
    </div>
  );
}
