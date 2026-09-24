import { useMutation } from "@tanstack/react-query";
import { toast } from "sonner";

import { revokeAutorun } from "@/shared/api/tauriWorkflows";
import { truncatePubkey } from "@/shared/lib/pubkey";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";

import {
  actionDescription,
  actionTriggerSummary,
  requiredCheckoutStepIds,
  runOnHostStepIds,
} from "../lib/actionDefinition";
import { autorunGrantState, standingGrantNote } from "../lib/autorunState";
import type { ProjectCodeRefTip } from "../lib/useProjectCodeRefTip";
import type { ProjectAction } from "../lib/useProjectActions";
import { ProjectActionRunControl } from "./ProjectActionRunControl";
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
  tip,
  onChanged,
}: {
  action: ProjectAction;
  /** The code repository's delivery tip, for the Run control's prefill. */
  tip: ProjectCodeRefTip | null;
  onChanged: () => void;
}) {
  const { workflow, runs, autorun, autorunError } = action;
  const grantState = autorunGrantState(autorun?.grants);
  const activeGrant = grantState.kind === "active" ? grantState : null;
  const staleGrant = grantState.kind === "stale";
  const grantNote = standingGrantNote(grantState, runs.length);
  const trigger = actionTriggerSummary(workflow.definition);
  const description = actionDescription(workflow.definition);
  const hostSteps = runOnHostStepIds(workflow.definition);
  const requiredSteps = requiredCheckoutStepIds(workflow.definition);
  const disabled = workflow.definition.enabled === false;

  const revoke = useMutation({
    mutationFn: () => {
      if (!workflow.channelId) {
        throw new Error("this workflow has no channel to revoke in");
      }
      return revokeAutorun(workflow.id, workflow.channelId);
    },
    onSuccess: () => {
      toast.success(`Autorun revoked for ${workflow.name}`);
      onChanged();
    },
    onError: (error: unknown) => {
      toast.error(`Revoke failed: ${errorSentence(error)}`);
    },
  });
  const { mutate: revokeMutate, isPending: revoking } = revoke;

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
            {activeGrant ? (
              <span
                className="flex items-center gap-1.5"
                data-testid="project-action-autorun-state"
              >
                <Badge variant="outline">
                  autorun · granted by {truncatePubkey(activeGrant.grantedBy)}
                </Badge>
                {grantNote ? (
                  <span
                    className="text-2xs text-muted-foreground"
                    data-testid="project-action-autorun-no-run-yet"
                  >
                    {grantNote}
                  </span>
                ) : null}
                <Button
                  data-testid="project-action-autorun-revoke"
                  disabled={revoking}
                  onClick={() => revokeMutate()}
                  size="sm"
                  type="button"
                  variant="ghost"
                >
                  Revoke
                </Button>
              </span>
            ) : staleGrant ? (
              <Badge variant="secondary">
                autorun granted for an earlier definition · approval re-armed
              </Badge>
            ) : null}
            {autorunError ? (
              <span>autorun state unreadable: {autorunError}</span>
            ) : null}
          </p>
          {description ? (
            <p className="mt-1 text-sm text-muted-foreground">{description}</p>
          ) : null}
        </div>
        <ProjectActionRunControl
          disabled={disabled}
          onRan={onChanged}
          requiredStepIds={requiredSteps}
          tip={tip}
          workflowId={workflow.id}
          workflowName={workflow.name}
        />
      </div>
      {action.runsError !== null ? (
        <p
          className="mt-3 text-xs text-destructive"
          data-testid="project-action-runs-unreadable"
          role="alert"
        >
          This action&apos;s runs could not be read: {action.runsError}. That is
          not the same as having no runs.
        </p>
      ) : runs.length === 0 ? (
        <p className="mt-3 text-xs text-muted-foreground">No runs yet.</p>
      ) : (
        <ul className="mt-3 divide-y divide-border/60 border-t border-border/60">
          {runs.map((entry) => (
            <ProjectActionRunRow
              action={action}
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
