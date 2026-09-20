import * as React from "react";
import { Play } from "lucide-react";
import { useMutation } from "@tanstack/react-query";
import { toast } from "sonner";

import { triggerWorkflow } from "@/shared/api/tauriWorkflows";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";

import { isFullCommitSha } from "../lib/actionDefinition";
import type { ProjectCodeRefTip } from "../lib/useProjectCodeRefTip";

function errorSentence(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** The sentence an action with no required checkout carries, verbatim. */
export const UNBOUND_RUN_SENTENCE =
  "This action does not require a commit: it will run in the project's recorded folder, in the working directory as found.";

/** What the field says when the relay has published no ref state to prefill. */
export function prefillSentence(tip: ProjectCodeRefTip | null): string {
  if (!tip) return "Reading the code repository's delivery ref…";
  if (tip.tip === null) return `No commit is offered — ${tip.reason}.`;
  return `Offered: ${tip.repositoryId ?? "the code repository"} ${tip.refName ?? ""} as the relay last observed it.`;
}

/**
 * Start a run, asking for the commit when the action demands one.
 *
 * Lane 184 gave a `run_on_host` step `checkout: required`, and the relay
 * refuses a manual trigger that names no commit — so before this control the
 * desktop could not start a verify-style action at all (ledger 184, "Owed").
 * The field is prefilled from the newest *relay-signed* ref state, editable,
 * and validated as a full 40-hex sha here so the operator is told before an
 * event is signed rather than after the relay refuses it.
 *
 * An action that requires no commit gets no field and a plain sentence
 * saying it runs in the working directory as found: a control that stayed
 * silent about that would be the lie ledger 178(g) reported.
 */
export function ProjectActionRunControl({
  workflowId,
  workflowName,
  requiredStepIds,
  disabled,
  tip,
  onRan,
}: {
  workflowId: string;
  workflowName: string;
  requiredStepIds: readonly string[];
  disabled: boolean;
  tip: ProjectCodeRefTip | null;
  onRan: () => void;
}) {
  const required = requiredStepIds.length > 0;
  const [commit, setCommit] = React.useState("");
  const [touched, setTouched] = React.useState(false);
  React.useEffect(() => {
    if (!touched && tip?.tip) setCommit(tip.tip);
  }, [tip?.tip, touched]);

  const run = useMutation({
    mutationFn: () =>
      triggerWorkflow(workflowId, required ? commit.trim() : null),
    onSuccess: (result) => {
      toast.success(`Run ${result.runId} queued for ${workflowName}`);
      onRan();
    },
    onError: (error: unknown) => {
      toast.error(`Run failed: ${errorSentence(error)}`);
    },
  });
  const { mutate, isPending } = run;
  const valid = !required || isFullCommitSha(commit);

  return (
    <div
      className="flex flex-col items-end gap-1"
      data-testid="project-action-run-control"
    >
      {required ? (
        <>
          <Input
            aria-label={`Commit for ${workflowName}`}
            className="w-[26rem] max-w-full font-mono text-xs"
            data-testid="project-action-run-commit"
            onChange={(event) => {
              setTouched(true);
              setCommit(event.target.value);
            }}
            placeholder="full 40-hex commit sha"
            spellCheck={false}
            value={commit}
          />
          <p
            className="text-2xs text-muted-foreground"
            data-testid="project-action-run-commit-note"
          >
            {`Step ${requiredStepIds.join(", ")} declares checkout: required. `}
            {prefillSentence(tip)}
          </p>
          {commit.trim().length > 0 && !valid ? (
            <p className="text-2xs text-destructive" role="alert">
              A bound commit must be a full 40-hex sha.
            </p>
          ) : null}
        </>
      ) : (
        <p
          className="max-w-[26rem] text-right text-2xs text-muted-foreground"
          data-testid="project-action-run-unbound-note"
        >
          {UNBOUND_RUN_SENTENCE}
        </p>
      )}
      <Button
        data-testid="project-action-run"
        disabled={disabled || isPending || !valid}
        onClick={() => mutate()}
        size="sm"
        type="button"
      >
        <Play />
        Run
      </Button>
    </div>
  );
}
