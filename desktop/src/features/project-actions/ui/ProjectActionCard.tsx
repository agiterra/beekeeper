import { Play } from "lucide-react";
import { useMutation } from "@tanstack/react-query";
import { toast } from "sonner";

import { revokeAutorun, triggerWorkflow } from "@/shared/api/tauriWorkflows";
import { truncatePubkey } from "@/shared/lib/pubkey";
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
  const { workflow, runs, autorun, autorunError } = action;
  const activeGrant =
    autorun?.grants.find(
      (grant) => grant.revokedAt === null && grant.matchesCurrent,
    ) ?? null;
  const staleGrant =
    !activeGrant &&
    (autorun?.grants.some(
      (grant) => grant.revokedAt === null && !grant.matchesCurrent,
    ) ??
      false);
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
