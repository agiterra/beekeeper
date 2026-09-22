import { useQuery } from "@tanstack/react-query";

import { relayClient } from "@/shared/api/relayClient";

import { HostStepApprovalInboxCard } from "@/features/project-actions/ui/HostStepApprovalInboxCard";
import {
  findApprovalRequestForRun,
  verifyConsentUnavailable,
  type ProjectVerifySetupResult,
} from "../lib/projectVerifySetup";

const KIND_APPROVAL_REQUESTED = 46010;
const POLL_MS = 2_000;

/**
 * Setup's one consent question (ledger 248): may this computer run the
 * project's `verify` definition, this exact one, from now on?
 *
 * The question is the existing kind:46010 of the one run setup started, and
 * the answer is the existing approval card — argv one row per argument, the
 * definition hash, the bound commit, and Approve disabled unless all of them
 * and the approver resolve (lane 218). Its "allow future runs of this exact
 * definition" is the standing grant; a changed definition hashes differently
 * and asks again, and a routine run of this one asks nobody.
 */
export function ProjectVerifyConsent({
  verify,
  verifyError,
}: {
  verify: ProjectVerifySetupResult | null;
  verifyError: string | null;
}) {
  const unavailable = verifyConsentUnavailable(verify, verifyError);
  const runId = verify?.runId ?? null;
  const channelId = verify?.channelId ?? null;
  const request = useQuery({
    queryKey: ["project-verify-consent", channelId, runId],
    enabled: unavailable === null,
    queryFn: async () => {
      const events = await relayClient.fetchEventsBatch([
        {
          kinds: [KIND_APPROVAL_REQUESTED],
          "#h": [channelId ?? ""],
          limit: 20,
        },
      ]);
      return findApprovalRequestForRun(events, runId);
    },
    refetchInterval: (query) => (query.state.data ? false : POLL_MS),
  });

  return (
    <div
      className="mt-3 flex flex-col gap-2"
      data-testid="project-verify-consent"
    >
      <p className="text-sm font-medium">Let this computer run verify</p>
      {unavailable !== null ? (
        <p
          className="text-xs text-destructive"
          data-testid="project-verify-consent-unavailable"
        >
          {unavailable}
        </p>
      ) : (
        <>
          <p className="text-xs text-muted-foreground">
            Agents run this project&apos;s verify action on your computer. The
            one answer below covers every future run of exactly this definition;
            an edited definition asks again. Allowing it also releases the run
            that asked, which tests the code seed commit{" "}
            <span className="font-mono">
              {(verify?.checkout ?? "").slice(0, 8)}
            </span>{" "}
            — a README and no tests yet — so that first result says nothing
            about your code.
          </p>
          {request.data ? (
            <HostStepApprovalInboxCard event={request.data} />
          ) : (
            <p
              className="text-xs text-muted-foreground"
              data-testid="project-verify-consent-waiting"
            >
              {request.error
                ? `The approval request could not be read: ${
                    request.error instanceof Error
                      ? request.error.message
                      : String(request.error)
                  }`
                : "Waiting for the relay's approval request…"}
            </p>
          )}
        </>
      )}
    </div>
  );
}
