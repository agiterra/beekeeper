import { CheckCircle2, ListChecks, XCircle } from "lucide-react";
import * as React from "react";

import { Button } from "@/shared/ui/button";

import {
  dismissProjectSetupOutcome,
  projectSetupIncomplete,
  projectSetupSteps,
  readProjectSetupOutcome,
  type ProjectSetupOutcome,
} from "../lib/projectSetupOutcome";
import type { ProjectContainer } from "../hooks";
import { ProjectAgentsInitAction } from "./ProjectAgentsInitAction";
import { ProjectVerifyConsent } from "./ProjectVerifyConsent";
import { SectionCard } from "./SectionCard";

/**
 * What creating this project actually did, kept until it is dismissed.
 *
 * Ledger 207(5): creation announces two repositories, seeds both, sets the
 * role source, installs the project's agents, clones and records the
 * checkout and adds the roster — and reported all of it in a toast that
 * vanished before it could be read. This card is that same report, on the
 * screen the new project lands on, with failures first and **Finish
 * repository setup** offered whenever anything is missing.
 *
 * It renders nothing for a project whose creation was never recorded here —
 * an older project, or one created on another computer — rather than
 * implying its setup is unknown.
 */
export function ProjectSetupCard({ project }: { project: ProjectContainer }) {
  const [outcome, setOutcome] = React.useState<ProjectSetupOutcome | null>(() =>
    readProjectSetupOutcome(project.address),
  );
  const [finishing, setFinishing] = React.useState(false);
  React.useEffect(() => {
    setOutcome(readProjectSetupOutcome(project.address));
    setFinishing(false);
  }, [project.address]);

  if (!outcome) return null;
  const steps = projectSetupSteps(outcome);
  const incomplete = projectSetupIncomplete(outcome);
  const failed = steps.filter((step) => step.failed).length;

  const dismiss = () => {
    dismissProjectSetupOutcome(project.address);
    setOutcome(null);
  };

  return (
    <SectionCard
      count={steps.length}
      countLabel={
        failed > 0 ? `${failed} of ${steps.length} incomplete` : "all done"
      }
      icon={<ListChecks className="size-4" />}
      testId="project-setup-card"
      title="Setup"
    >
      <ul className="flex flex-col gap-1" data-testid="project-setup-steps">
        {steps.map((step) => (
          <li
            className="flex items-start gap-2 text-xs"
            data-failed={step.failed ? "true" : "false"}
            data-testid={`project-setup-step-${step.id}`}
            key={step.id}
          >
            {step.failed ? (
              <XCircle className="mt-0.5 size-3.5 shrink-0 text-destructive" />
            ) : (
              <CheckCircle2 className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
            )}
            <span className="min-w-0 flex-1">
              <span className="font-medium">{step.label}</span>
              <span
                className={
                  step.failed
                    ? "block text-destructive"
                    : "block text-muted-foreground"
                }
              >
                {step.detail}
              </span>
            </span>
          </li>
        ))}
      </ul>
      {outcome.result?.seededActionsYml ? (
        <ProjectVerifyConsent
          verify={outcome.verify ?? null}
          verifyError={outcome.verifyError ?? null}
        />
      ) : null}
      {finishing ? (
        <div className="mt-2">
          <ProjectAgentsInitAction
            onCancel={() => setFinishing(false)}
            onRan={() => setOutcome(readProjectSetupOutcome(project.address))}
            projectRef={project.address}
            projectSlug={project.dtag}
          />
        </div>
      ) : (
        <div className="mt-3 flex gap-2">
          {incomplete ? (
            <Button
              data-testid="project-setup-finish"
              onClick={() => setFinishing(true)}
              size="sm"
            >
              Finish repository setup
            </Button>
          ) : null}
          <Button
            data-testid="project-setup-dismiss"
            onClick={dismiss}
            size="sm"
            variant="ghost"
          >
            Dismiss
          </Button>
        </div>
      )}
    </SectionCard>
  );
}
