import {
  PROJECT_TEAM_SETUP_STEP_LABELS,
  PROJECT_TEAM_SETUP_STEPS,
  projectTeamSetupStepIndex,
  type ProjectTeamSetupStage,
} from "../lib/projectTeamSetupStage";

const CURRENT_STATE_LABEL: Record<ProjectTeamSetupStage["state"], string> = {
  todo: "current step",
  checking: "checking",
  working: "in progress",
  uncertain: "not confirmed",
  blocked: "blocked",
  done: "done",
};

/** Five steps in order, the current one marked with its honest state. */
export function ProjectTeamSetupStepper({
  stage,
}: {
  stage: ProjectTeamSetupStage;
}) {
  const current = projectTeamSetupStepIndex(stage.step);
  return (
    <ol
      aria-label="Setup steps"
      className="flex flex-wrap gap-x-3 gap-y-1 text-xs"
      data-testid="project-team-setup-stepper"
    >
      {PROJECT_TEAM_SETUP_STEPS.map((step, index) => {
        const isCurrent = index === current;
        const status = isCurrent
          ? CURRENT_STATE_LABEL[stage.state]
          : index < current
            ? "done"
            : "not started";
        return (
          <li
            aria-current={isCurrent ? "step" : undefined}
            className={
              isCurrent
                ? stage.state === "blocked"
                  ? "font-semibold text-destructive"
                  : "font-semibold text-foreground"
                : "text-muted-foreground"
            }
            data-state={isCurrent ? stage.state : status}
            key={step}
          >
            <span aria-hidden>
              {index < current || (isCurrent && stage.state === "done")
                ? "✓ "
                : `${index + 1}. `}
            </span>
            {PROJECT_TEAM_SETUP_STEP_LABELS[step]}
            <span className="sr-only"> ({status})</span>
          </li>
        );
      })}
    </ol>
  );
}
