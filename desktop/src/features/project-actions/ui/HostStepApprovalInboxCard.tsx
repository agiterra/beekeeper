import { useQuery } from "@tanstack/react-query";

import { getWorkflowRun } from "@/shared/api/tauriWorkflows";

import {
  buildHostStepApprovalView,
  readHostStepApprovalRequest,
} from "../lib/hostStepApproval";
import {
  matchBoundDefinition,
  readBoundDefinition,
} from "../lib/resolveBoundDefinition";
import { useApprovalAuthority } from "../lib/useApprovalAuthority";
import { ProjectActionApprovalCard } from "./ProjectActionApprovalCard";

/** React Query key for one parked approval's run and bound definition. */
export function hostStepApprovalQueryKey(approvalRef: string) {
  return ["host-step-approval", approvalRef] as const;
}

/**
 * A kind:46010 in the inbox, rendered as the approval it is (ledger 171(b)).
 *
 * Two rules this card exists to hold, both from Astra's Wave 2 review:
 *
 * - **What is shown is what is approved.** The command comes from the
 *   definition whose hash equals the run's binding, never from whatever is
 *   current; when they differ, or either read fails, no command is shown and
 *   Approve is unavailable. Deny stays available to an approver, because
 *   refusing what you cannot fully see is always a safe answer (finding 1).
 * - **The approver is the project's**, resolved from the `approverSpec`'s
 *   coordinate, never from the publisher's key or the `p` tag (finding 9).
 *
 * The bound commit is read from the **run** (lane 206), so a run awaiting its
 * first approval — which has no host result yet and never will until it is
 * approved — still names the commit it would test (finding 10).
 *
 * Renders `null` for an event whose shape this reader does not fully
 * recognise, so the ordinary message body still shows: a half-read request
 * must never carry an Approve button.
 */
export function HostStepApprovalInboxCard({
  event,
}: {
  event: {
    kind: number;
    tags: readonly (readonly string[])[];
    content: string;
  };
}) {
  const request = readHostStepApprovalRequest(event);
  const detail = useQuery({
    queryKey: hostStepApprovalQueryKey(request?.approvalRef ?? ""),
    enabled: request !== null,
    queryFn: async () => {
      const runId = request?.runId ?? "";
      const workflowId = request?.workflowId ?? "";
      const [run, definition] = await Promise.all([
        getWorkflowRun(runId).then(
          (value) => ({ value, error: null as string | null }),
          (error: unknown) => ({
            value: null,
            error: error instanceof Error ? error.message : String(error),
          }),
        ),
        readBoundDefinition(workflowId),
      ]);
      return { run, definition };
    },
  });
  const authority = useApprovalAuthority(request?.approverSpec ?? null);

  if (request === null) return null;

  const run = detail.data?.run.value ?? null;
  const read = detail.data?.definition ?? null;
  const view = buildHostStepApprovalView({
    request,
    workflowName: read?.read?.name ?? null,
    runDefinitionHash: run?.run.definitionHash ?? null,
    runRead: run !== null,
    // R1: body and hash from the one read, never joined across two.
    definition: matchBoundDefinition({
      runHash: run?.run.definitionHash ?? null,
      read: read ?? { read: null, error: null },
      stepId: request.stepId,
    }),
    // From the run, not from a host result: the result cannot exist yet.
    checkout: run?.run.checkout ?? { state: "not-reported" },
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
