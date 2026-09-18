import * as React from "react";
import { useMutation } from "@tanstack/react-query";
import { toast } from "sonner";

import { denyApproval, grantApproval } from "@/shared/api/tauriWorkflows";
import { formatItemTimestamp } from "@/shared/lib/datetime";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import { Checkbox } from "@/shared/ui/checkbox";

import { type ActionRunTone, describeActionRun } from "../lib/actionRunLabel";
import type { ProjectActionRun } from "../lib/useProjectActions";

const TONE_CLASS: Record<ActionRunTone, string> = {
  pending: "text-amber-600 dark:text-amber-400",
  ok: "text-emerald-600 dark:text-emerald-400",
  bad: "text-destructive",
  muted: "text-muted-foreground",
};

/** Spec § 5.4: the checkbox that turns a grant into an autorun grant. */
export const AUTORUN_GRANT_LABEL = "allow future runs of this action";

function formatSince(unixSeconds: number): string {
  return formatItemTimestamp(unixSeconds, { withTime: true });
}

function errorSentence(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * One run of an action: the sentence its records prove, and — when a live
 * approval is waiting — Approve / Deny.
 */
export function ProjectActionRunRow({
  entry,
  onChanged,
}: {
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

  const [allowFutureRuns, setAllowFutureRuns] = React.useState(false);
  const decide = useMutation({
    mutationFn: async (input: {
      action: "grant" | "deny";
      ref: string;
      allowFutureRuns: boolean;
    }) =>
      input.action === "grant"
        ? grantApproval(
            input.ref,
            undefined,
            input.allowFutureRuns ? "action" : "run",
          )
        : denyApproval(input.ref),
    onSuccess: (_data, input) => {
      toast.success(
        input.action === "deny"
          ? "Denied"
          : input.allowFutureRuns
            ? "Approved, and future runs of this definition"
            : "Approved",
      );
      onChanged();
    },
    onError: (error: unknown) => {
      toast.error(`Approval failed: ${errorSentence(error)}`);
    },
  });
  const { mutate: decideMutate, isPending: deciding } = decide;

  const autorunId = React.useId();
  const pending = row.pendingApproval;
  return (
    <li
      className="flex flex-wrap items-center gap-x-3 gap-y-1 py-1.5 text-sm"
      data-run-id={entry.run.id}
      data-testid="project-action-run"
    >
      <span className="text-2xs tabular-nums text-muted-foreground">
        {formatSince(entry.run.createdAt)}
      </span>
      <span className={cn("min-w-0 break-words", TONE_CLASS[row.tone])}>
        {row.label}
        {entry.hostStepsError ? (
          <span className="text-muted-foreground">
            {" "}
            · host steps unreadable: {entry.hostStepsError}
          </span>
        ) : null}
        {entry.hostSteps.some((step) => step.artifacts.length > 0) ? (
          <span className="text-muted-foreground">
            {" · logs: "}
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
      </span>
      {pending ? (
        <span className="ml-auto flex items-center gap-2">
          <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
            <Checkbox
              checked={allowFutureRuns}
              data-testid="project-action-autorun"
              id={autorunId}
              onCheckedChange={(checked) =>
                setAllowFutureRuns(checked === true)
              }
            />
            <label htmlFor={autorunId}>{AUTORUN_GRANT_LABEL}</label>
          </span>
          <Button
            data-testid="project-action-approve"
            disabled={deciding}
            onClick={() =>
              decideMutate({
                action: "grant",
                ref: pending.approvalRef,
                allowFutureRuns,
              })
            }
            size="sm"
            type="button"
          >
            Approve
          </Button>
          <Button
            data-testid="project-action-deny"
            disabled={deciding}
            onClick={() =>
              decideMutate({
                action: "deny",
                ref: pending.approvalRef,
                allowFutureRuns: false,
              })
            }
            size="sm"
            type="button"
            variant="outline"
          >
            Deny
          </Button>
        </span>
      ) : null}
    </li>
  );
}
