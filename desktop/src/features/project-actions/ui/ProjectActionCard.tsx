import { Play } from "lucide-react";
import { useMutation } from "@tanstack/react-query";
import { toast } from "sonner";

import { triggerWorkflow } from "@/shared/api/tauriWorkflows";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";

import {
  actionDescription,
  actionTriggerSummary,
  runOnHostStepIds,
} from "../lib/actionDefinition";
import type { ProjectAction } from "../lib/useProjectActions";
import { ProjectActionRunRow } from "./ProjectActionRunRow";

function errorSentence(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * One action: its name, trigger and whether it runs on a host; a Run button
 * that triggers the workflow by id; and its latest runs, each described by
 * what the relay's records prove.
 */
export function ProjectActionCard({
  action,
  onChanged,
}: {
  action: ProjectAction;
  onChanged: () => void;
}) {
  const { workflow, runs } = action;
  const trigger = actionTriggerSummary(workflow.definition);
  const description = actionDescription(workflow.definition);
  const hostSteps = runOnHostStepIds(workflow.definition);
  const disabled = workflow.definition.enabled === false;

  const run = useMutation({
    mutationFn: () => triggerWorkflow(workflow.id),
    onSuccess: (result) => {
      toast.success(`Run ${result.runId} queued for ${workflow.name}`);
      onChanged();
    },
    onError: (error: unknown) => {
      toast.error(`Run failed: ${errorSentence(error)}`);
    },
  });
  const { mutate: runMutate, isPending: running } = run;

  return (
    <section
      className="rounded-xl border border-border/70 bg-card p-4 shadow-xs"
      data-testid="project-action-card"
      data-workflow-id={workflow.id}
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 className="break-words text-base font-semibold text-foreground">
            {workflow.name}
          </h2>
          <p className="mt-0.5 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
            {trigger ? <span>{trigger}</span> : <span>No trigger</span>}
            {hostSteps.length > 0 ? (
              <Badge variant="outline">
                run_on_host · {hostSteps.join(", ")}
              </Badge>
            ) : null}
            {disabled ? <Badge variant="secondary">disabled</Badge> : null}
          </p>
          {description ? (
            <p className="mt-1 text-sm text-muted-foreground">{description}</p>
          ) : null}
        </div>
        <Button
          data-testid="project-action-run"
          disabled={running || disabled}
          onClick={() => runMutate()}
          size="sm"
          type="button"
        >
          <Play />
          Run
        </Button>
      </div>
      {runs.length === 0 ? (
        <p className="mt-3 text-xs text-muted-foreground">No runs yet.</p>
      ) : (
        <ul className="mt-3 divide-y divide-border/60 border-t border-border/60">
          {runs.map((entry) => (
            <ProjectActionRunRow
              entry={entry}
              key={entry.run.id}
              onChanged={onChanged}
            />
          ))}
        </ul>
      )}
    </section>
  );
}
