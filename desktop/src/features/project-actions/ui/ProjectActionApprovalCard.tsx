import { useMutation } from "@tanstack/react-query";
import { toast } from "sonner";

import { denyApproval, grantApproval } from "@/shared/api/tauriWorkflows";
import { formatItemTimestamp } from "@/shared/lib/datetime";
import { Button } from "@/shared/ui/button";

import type {
  ApprovalFact,
  HostStepApprovalView,
} from "../lib/hostStepApproval";

/** The three answers, in the words the buttons carry. */
export const APPROVE_ONCE_LABEL = "Approve once";
export const APPROVE_ACTION_LABEL =
  "Approve and allow future runs of this exact definition";
export const DENY_LABEL = "Deny";

function errorSentence(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * The command, as argument rows plus the JSON array beside them.
 *
 * Never one space-joined line: `["sh","-c","a b"]` must not be able to read
 * as four arguments (finding 1). The numbered rows show the split and the
 * JSON line shows it unambiguously in a form that can be compared by eye
 * with the definition.
 */
function CommandFact({ fact }: { fact: ApprovalFact }) {
  const rows = fact.argumentRows;
  if (fact.value === null || !rows) {
    return (
      <div
        className="flex flex-col gap-0.5"
        data-testid="host-step-approval-command"
      >
        <span className="text-2xs uppercase tracking-wide text-muted-foreground">
          Command
        </span>
        <span
          className="text-xs text-muted-foreground"
          data-testid="host-step-approval-command-unavailable"
        >
          not shown — {fact.reason}
        </span>
      </div>
    );
  }
  return (
    <div
      className="flex flex-col gap-0.5 sm:col-span-2"
      data-testid="host-step-approval-command"
    >
      <span className="text-2xs uppercase tracking-wide text-muted-foreground">
        Command · {rows.length} argument
        {rows.length === 1 ? "" : "s"}
      </span>
      <ol className="flex flex-col gap-0.5">
        {rows.map((row) => (
          <li
            className="flex gap-2 font-mono text-xs"
            data-testid="host-step-approval-argument"
            key={row.key}
          >
            <span className="shrink-0 text-muted-foreground">{row.index}</span>
            <span className="break-all">{row.value}</span>
          </li>
        ))}
      </ol>
      {fact.json ? (
        <span className="break-all font-mono text-2xs text-muted-foreground">
          {fact.json}
        </span>
      ) : null}
    </div>
  );
}

function Fact({
  label,
  fact,
  mono = false,
  testid,
}: {
  label: string;
  fact: ApprovalFact;
  mono?: boolean;
  testid: string;
}) {
  return (
    <div className="flex flex-col gap-0.5" data-testid={testid}>
      <span className="text-2xs uppercase tracking-wide text-muted-foreground">
        {label}
      </span>
      {fact.value === null ? (
        <span
          className="text-xs text-muted-foreground"
          data-testid={`${testid}-unavailable`}
        >
          not established — {fact.reason}
        </span>
      ) : (
        <span
          className={
            mono ? "break-all font-mono text-xs" : "break-words text-sm"
          }
        >
          {fact.value}
        </span>
      )}
    </div>
  );
}

/**
 * A parked host-step approval as a card, not as raw JSON (ledger 171(b)).
 *
 * The card states what is about to run on this computer — the action, the
 * step, the definition the run is bound to, the commit, and the exact command
 * out of the published definition — before offering any answer, and names
 * every fact it could not establish rather than leaving it blank.
 *
 * `canApprove` false renders the same facts with no control and the sentence
 * that says who may answer: a read-only viewer is never offered a write, and
 * this surface mints nothing on render.
 */
export function ProjectActionApprovalCard({
  view,
  canApprove,
  authoritySentence,
  onAnswered,
}: {
  view: HostStepApprovalView;
  canApprove: boolean;
  authoritySentence: string;
  onAnswered?: () => void;
}) {
  const answer = useMutation({
    mutationFn: async (input: { kind: "once" | "action" | "deny" }) =>
      input.kind === "deny"
        ? denyApproval(view.approvalRef)
        : grantApproval(
            view.approvalRef,
            undefined,
            input.kind === "action" ? "action" : "run",
          ),
    onSuccess: (_result, input) => {
      toast.success(
        input.kind === "deny"
          ? `Denied ${view.actionName} · ${view.stepId}`
          : input.kind === "action"
            ? "Approved, and future runs of this exact definition"
            : "Approved this run only",
      );
      onAnswered?.();
    },
    onError: (error: unknown) => {
      toast.error(`Could not answer: ${errorSentence(error)}`);
    },
  });
  const { mutate, isPending } = answer;

  return (
    <section
      className="rounded-xl border border-amber-500/40 bg-amber-500/5 p-4"
      data-approval-ref={view.approvalRef}
      data-testid="host-step-approval-card"
    >
      <header className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
        <h3 className="break-words text-base font-semibold text-foreground">
          {view.actionName}
        </h3>
        <span className="text-sm text-muted-foreground">
          step <span className="font-mono">{view.stepId}</span>
          {view.stepIndex === null ? null : ` (index ${view.stepIndex})`}
        </span>
        <span className="ml-auto text-2xs text-muted-foreground">
          {view.expiresAt === null
            ? "no expiry recorded"
            : `expires ${formatItemTimestamp(view.expiresAt, { withTime: true })}`}
        </span>
      </header>
      {view.message ? (
        <p className="mt-2 text-sm text-foreground">{view.message}</p>
      ) : null}
      <div className="mt-3 grid gap-3 sm:grid-cols-2">
        <CommandFact fact={view.command} />
        <Fact
          fact={view.boundCommit}
          label="Commit"
          mono
          testid="host-step-approval-commit"
        />
        <Fact
          fact={view.definitionHash}
          label="Definition"
          mono
          testid="host-step-approval-definition"
        />
        <div className="flex flex-col gap-0.5">
          <span className="text-2xs uppercase tracking-wide text-muted-foreground">
            Run
          </span>
          <span className="break-all font-mono text-xs">{view.runId}</span>
        </div>
      </div>
      <p
        className="mt-3 text-xs text-muted-foreground"
        data-testid="host-step-approval-authority"
      >
        {authoritySentence}
      </p>
      {canApprove && !view.grantAvailable ? (
        <p
          className="mt-3 text-xs text-amber-600 dark:text-amber-400"
          data-testid="host-step-approval-grant-blocked"
          role="status"
        >
          Approve is unavailable: {view.grantBlockedReason} You can still deny
          this request.
        </p>
      ) : null}
      {canApprove ? (
        <div className="mt-3 flex flex-wrap gap-2">
          <Button
            data-testid="host-step-approve-once"
            disabled={isPending || !view.grantAvailable}
            onClick={() => mutate({ kind: "once" })}
            size="sm"
            type="button"
          >
            {APPROVE_ONCE_LABEL}
          </Button>
          <Button
            data-testid="host-step-approve-action"
            disabled={isPending || !view.grantAvailable}
            onClick={() => mutate({ kind: "action" })}
            size="sm"
            type="button"
            variant="outline"
          >
            {APPROVE_ACTION_LABEL}
          </Button>
          <Button
            data-testid="host-step-deny"
            disabled={isPending}
            onClick={() => mutate({ kind: "deny" })}
            size="sm"
            type="button"
            variant="ghost"
          >
            {DENY_LABEL}
          </Button>
        </div>
      ) : null}
    </section>
  );
}
