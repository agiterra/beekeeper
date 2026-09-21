import * as React from "react";

import { formatItemTimestamp } from "@/shared/lib/datetime";
import { cn } from "@/shared/lib/cn";

import { type ActionRunTone, describeActionRun } from "../lib/actionRunLabel";
import { runProvenanceFacts } from "../lib/actionRunProvenance";
import { buildHostStepApprovalView } from "../lib/hostStepApproval";
import { matchBoundDefinition } from "../lib/resolveBoundDefinition";
import { useApprovalAuthority } from "../lib/useApprovalAuthority";
import type { ProjectAction, ProjectActionRun } from "../lib/useProjectActions";
import { ProjectActionApprovalCard } from "./ProjectActionApprovalCard";

const TONE_CLASS: Record<ActionRunTone, string> = {
  pending: "text-amber-600 dark:text-amber-400",
  ok: "text-emerald-600 dark:text-emerald-400",
  bad: "text-destructive",
  muted: "text-muted-foreground",
};

/** Spec § 5.4: what an action-scoped grant releases, in the card's words. */
export const AUTORUN_GRANT_LABEL =
  "Approve and allow future runs of this exact definition";

function formatSince(unixSeconds: number): string {
  return formatItemTimestamp(unixSeconds, { withTime: true });
}

function rfc3339ToSeconds(value: string | null): number | null {
  if (!value) return null;
  const millis = Date.parse(value);
  return Number.isFinite(millis) ? Math.floor(millis / 1_000) : null;
}

/**
 * One run of an action: the sentence its records prove, the provenance those
 * records carry, and — when an approval is parked — the card that states what
 * would run before offering an answer (ledger 171(b)).
 */
export function ProjectActionRunRow({
  action,
  entry,
  onChanged,
}: {
  action: ProjectAction;
  entry: ProjectActionRun;
  onChanged: () => void;
}) {
  const row = React.useMemo(
    () =>
      describeActionRun(entry.run, entry.approvals, entry.hostSteps, {
        formatTime: formatSince,
      }),
    [entry],
  );
  const facts = React.useMemo(
    () => runProvenanceFacts(entry.run, entry.hostSteps, entry.hostStepsError),
    [entry],
  );
  const pending = row.pendingApproval;
  // The one resolver, the same one the inbox card uses: authority is the
  // project's, read from the request's own `approverSpec` (finding 9).
  const authority = useApprovalAuthority(pending?.approverSpec ?? null);
  // Finding 1, and R1 after it: the definition put in front of the approver
  // must be the one the run is bound to, and its hash must be of the bytes
  // shown — so both come from `get_workflow_definition`, which hashes what it
  // returns with the relay's own canonical function.
  const approvalView = React.useMemo(() => {
    if (!pending) return null;
    return buildHostStepApprovalView({
      request: {
        approvalRef: pending.approvalRef,
        runId: pending.runId,
        workflowName: action.workflow.name,
        stepId: pending.stepId,
        stepIndex: pending.stepIndex,
        approverSpec: pending.approverSpec,
        message: null,
        expiresAt: rfc3339ToSeconds(pending.expiresAt),
      },
      workflowName: action.workflow.name,
      runDefinitionHash: entry.run.definitionHash,
      runRead: true,
      // R1: from the one read that carries both, never from the list read's
      // body beside the autorun read's hash.
      definition: matchBoundDefinition({
        runHash: entry.run.definitionHash,
        read: action.boundDefinition,
        stepId: pending.stepId,
      }),
      // Finding 10: off the run, not off a host result that cannot exist
      // until this very approval is granted.
      checkout: entry.run.checkout,
    });
  }, [
    action.boundDefinition,
    action.workflow,
    entry.run.checkout,
    entry.run.definitionHash,
    pending,
  ]);

  return (
    <li
      className="flex flex-col gap-2 py-2 text-sm"
      data-run-id={entry.run.id}
      data-testid="project-action-run"
    >
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <span className="text-2xs tabular-nums text-muted-foreground">
          {formatSince(entry.run.createdAt)}
        </span>
        <span className={cn("min-w-0 break-words", TONE_CLASS[row.tone])}>
          {row.label}
        </span>
        {entry.hostSteps.some((step) => step.artifacts.length > 0) ? (
          <span className="text-xs text-muted-foreground">
            {"logs: "}
            {entry.hostSteps
              .flatMap((step) => step.artifacts)
              .map((artifact, index) => (
                <React.Fragment key={artifact.url}>
                  {index > 0 ? ", " : null}
                  <a
                    className="underline"
                    data-testid="project-action-artifact"
                    href={artifact.url}
                    rel="noreferrer"
                    target="_blank"
                  >
                    {artifact.name}
                  </a>
                </React.Fragment>
              ))}
          </span>
        ) : null}
      </div>
      <dl
        className="grid gap-x-4 gap-y-1 sm:grid-cols-2"
        data-testid="project-action-run-provenance"
      >
        {facts.map((fact) => (
          <div className="flex flex-wrap items-baseline gap-2" key={fact.label}>
            <dt className="text-2xs uppercase tracking-wide text-muted-foreground">
              {fact.label}
            </dt>
            <dd
              className={cn(
                "min-w-0 break-all",
                fact.value === null
                  ? "text-xs text-muted-foreground"
                  : fact.mono
                    ? "font-mono text-xs"
                    : "text-xs",
              )}
            >
              {fact.value ?? `not established — ${fact.reason}`}
            </dd>
          </div>
        ))}
      </dl>
      {approvalView ? (
        <ProjectActionApprovalCard
          authoritySentence={authority.sentence}
          canApprove={authority.canApprove}
          onAnswered={onChanged}
          view={approvalView}
        />
      ) : null}
    </li>
  );
}
