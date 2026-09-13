import { Button } from "@/shared/ui/button";
import {
  projectTeamSetupRecordedLine,
  type ProjectTeamSetupSummary,
} from "../lib/useProjectTeamSetupSummary";

/** Says whether a draft exists before anyone opens setup. */
export function ProjectTeamSetupOpenButton({
  summary,
  failed,
  unavailable,
  onOpen,
}: {
  /** `undefined` while the read is pending; `null` when no draft is saved. */
  summary: ProjectTeamSetupSummary | undefined;
  failed: boolean;
  unavailable: string | null;
  onOpen: () => void;
}) {
  const status = unavailable
    ? unavailable
    : summary
      ? projectTeamSetupRecordedLine(summary)
      : failed
        ? "Couldn't check for a saved draft. Opening setup checks again."
        : null;
  return (
    <>
      <Button
        data-testid="project-team-setup-open"
        disabled={Boolean(unavailable)}
        onClick={onOpen}
        type="button"
        variant="outline"
      >
        {summary ? "Continue project role setup" : "Set up project roles"}
      </Button>
      {status ? (
        <p
          className="mt-1 text-sm text-muted-foreground"
          data-testid="project-team-setup-status"
        >
          {status}
        </p>
      ) : null}
    </>
  );
}
