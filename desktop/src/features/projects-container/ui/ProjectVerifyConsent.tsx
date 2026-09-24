import { useMutation } from "@tanstack/react-query";

import { grantStandingApproval } from "@/shared/api/tauriWorkflows";
import { Button } from "@/shared/ui/button";

import {
  verifyConsentUnavailable,
  type ProjectVerifySetupResult,
} from "../lib/projectVerifySetup";

function errorSentence(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Setup's one consent question (ledger 248, 252; spec § 5.4): may this
 * computer run the project's `verify` definition, this exact one, from now
 * on?
 *
 * Control run 6 (2026-09-24) found the prior card answered a kind:46010 that
 * setup manufactured by starting a real run at the code repository's empty
 * seed commit — a run on a provider setup never starts, so it sat unclaimed
 * and then ran red on a commit with no tests. The click now publishes a
 * standing grant directly (`grantStandingApproval`), bound to
 * `(workflowId, definitionHash)`. No run happens here; a routine run of this
 * exact definition, from any seat, is what the grant later covers — and asks
 * nobody. An edited definition hashes differently and asks again.
 */
export function ProjectVerifyConsent({
  verify,
  verifyError,
}: {
  verify: ProjectVerifySetupResult | null;
  verifyError: string | null;
}) {
  const unavailable = verifyConsentUnavailable(verify, verifyError);
  const workflowId = verify?.workflowId ?? null;
  const definitionHash = verify?.definitionHash ?? null;

  const grant = useMutation({
    mutationFn: () => {
      if (workflowId === null || definitionHash === null) {
        throw new Error("no definition to grant yet");
      }
      return grantStandingApproval(workflowId, definitionHash);
    },
  });
  const { mutate: grantMutate, isPending, isSuccess, error, data } = grant;

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
            an edited definition asks again. No run happens as part of answering
            — the first run of it is whatever a session starts next.
          </p>
          {isSuccess ? (
            <p
              className="text-xs text-muted-foreground"
              data-testid="project-verify-consent-granted"
            >
              Standing grant published (event{" "}
              <span className="font-mono">{data.eventId.slice(0, 8)}</span>
              ). No run has happened yet.
            </p>
          ) : (
            <>
              <Button
                data-testid="project-verify-consent-approve"
                disabled={
                  isPending || workflowId === null || definitionHash === null
                }
                onClick={() => grantMutate()}
                size="sm"
                type="button"
              >
                Approve and allow future runs
              </Button>
              {error ? (
                <p
                  className="text-xs text-destructive"
                  data-testid="project-verify-consent-error"
                >
                  {`The grant was not published: ${errorSentence(error)}`}
                </p>
              ) : null}
            </>
          )}
        </>
      )}
    </div>
  );
}
