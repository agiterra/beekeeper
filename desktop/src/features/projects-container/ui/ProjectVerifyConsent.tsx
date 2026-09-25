import { useMutation } from "@tanstack/react-query";

import { grantStandingApproval } from "@/shared/api/tauriWorkflows";
import { Button } from "@/shared/ui/button";

import {
  verifyConsentCase,
  verifyConsentCaseSentence,
  verifyConsentUnavailable,
  type ProjectVerifySetupResult,
} from "../lib/projectVerifySetup";

function errorSentence(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * The consent question `project_verify_setup` did not already answer
 * (Brian's 2026-09-25 ruling: **creation is consent**). Creating this
 * project, on this computer, with the owner's key, already published the
 * standing grant right after the definition itself — so this card renders
 * `null` the moment `verify.grantEventId` is set. It appears only in the two
 * cases that survive that: this host did not create the project (no grant
 * exists for this key against the current hash), or the definition's hash
 * changed since the grant this host holds. [`verifyConsentCase`] decides
 * which; [`verifyConsentCaseSentence`] names it.
 *
 * Control run 6 (2026-09-24) found the prior card answered a kind:46010 that
 * setup manufactured by starting a real run at the code repository's empty
 * seed commit — a run on a provider setup never starts, so it sat unclaimed
 * and then ran red on a commit with no tests. Run 7 (2026-09-25) then found
 * that asking a second time for consent creation itself already gives was
 * its own kind of dishonesty; setup now grants automatically, and this card
 * is the fallback for the two cases it cannot answer for itself.
 */
export function ProjectVerifyConsent({
  verify,
  verifyError,
}: {
  verify: ProjectVerifySetupResult | null;
  verifyError: string | null;
}) {
  const unavailable = verifyConsentUnavailable(verify, verifyError);
  const consentCase = verifyConsentCase(verify);
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

  if (unavailable === null && consentCase === null) return null;

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
          <p
            className="text-xs text-muted-foreground"
            data-testid="project-verify-consent-case"
          >
            {consentCase !== null ? verifyConsentCaseSentence(consentCase) : ""}
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
