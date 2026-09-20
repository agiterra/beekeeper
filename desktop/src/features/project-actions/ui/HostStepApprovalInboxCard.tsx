import { useQuery } from "@tanstack/react-query";

import { useIdentityQuery } from "@/shared/api/hooks";
import { getWorkflow, getWorkflowRun } from "@/shared/api/tauriWorkflows";

import { hostStepCommand } from "../lib/actionDefinition";
import {
  approvalAuthority,
  buildHostStepApprovalView,
  readHostStepApprovalRequest,
} from "../lib/hostStepApproval";
import { ProjectActionApprovalCard } from "./ProjectActionApprovalCard";

/** React Query key for one parked approval's run and definition. */
export function hostStepApprovalQueryKey(approvalRef: string) {
  return ["host-step-approval", approvalRef] as const;
}

/**
 * A kind:46010 in the inbox, rendered as the approval it is (ledger 171(b)).
 *
 * Before this the inbox printed the request's raw JSON under a reply
 * composer, so the only way to answer a command about to run on this computer
 * was to hand-sign a kind:46030. The event alone carries neither the
 * definition the run is bound to nor the command, so this reads the run
 * (`GET /workflow-runs/{run_id}`, lane 190) and the published definition and
 * hands both to the card — which states every fact it could not establish
 * rather than leaving it blank.
 *
 * Renders `null` for an event whose shape this reader does not fully
 * recognise, so the ordinary message body still shows: a half-read request
 * must never carry an Approve button.
 */
export function HostStepApprovalInboxCard({
  event,
  /** The project owner, when the caller knows one; authority is theirs. */
  projectOwner = null,
}: {
  event: {
    kind: number;
    tags: readonly (readonly string[])[];
    content: string;
  };
  projectOwner?: string | null;
}) {
  const request = readHostStepApprovalRequest(event);
  const identity = useIdentityQuery();
  const detail = useQuery({
    queryKey: hostStepApprovalQueryKey(request?.approvalRef ?? ""),
    enabled: request !== null,
    queryFn: async () => {
      const runId = request?.runId ?? "";
      const workflowId = request?.workflowId ?? "";
      const [run, workflow] = await Promise.all([
        getWorkflowRun(runId).then(
          (value) => ({ value, error: null as string | null }),
          (error: unknown) => ({
            value: null,
            error: error instanceof Error ? error.message : String(error),
          }),
        ),
        getWorkflow(workflowId).then(
          (value) => ({ value, error: null as string | null }),
          (error: unknown) => ({
            value: null,
            error: error instanceof Error ? error.message : String(error),
          }),
        ),
      ]);
      return { run, workflow };
    },
  });

  if (request === null) return null;

  const run = detail.data?.run.value ?? null;
  const workflow = detail.data?.workflow.value ?? null;
  const boundCommit =
    run?.hostSteps.find((step) => step.stepId === request.stepId)?.checkout
      ?.sha ?? null;
  const view = buildHostStepApprovalView({
    request,
    workflowName: workflow?.name ?? null,
    runDefinitionHash: run?.run.definitionHash ?? null,
    runRead: run !== null,
    command: workflow
      ? hostStepCommand(workflow.definition, request.stepId)
      : null,
    definitionRead: workflow !== null,
    boundCommit,
  });
  // The owner the request itself attributes to (`p`) is the workflow owner,
  // which for a project action is the project's own writer. It is the best
  // authority fact this surface holds without a project read.
  const owner =
    projectOwner ?? event.tags.find((tag) => tag[0] === "p")?.[1] ?? null;
  const authority = approvalAuthority({
    viewerPubkey: identity.data?.pubkey ?? null,
    projectOwner: owner,
    approverSpec: request.approverSpec,
  });

  return (
    <ProjectActionApprovalCard
      authoritySentence={authority.sentence}
      canApprove={authority.canApprove}
      onAnswered={() => void detail.refetch()}
      view={view}
    />
  );
}
