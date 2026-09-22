import { useCodingSessionDecisionAnswer } from "@/features/coding-sessions/hooks/useCodingSessionDecisionAnswer";
import { CodingSessionDecisionAnswerForm } from "@/features/coding-sessions/ui/CodingSessionDecisionAnswerForm";
import { useIdentityQuery } from "@/shared/api/hooks";

import {
  inboxDecisionModel,
  inboxDecisionWakeDisclosure,
  type InboxDecisionRequest,
} from "../lib/decisionRequestInbox";

/**
 * A kind:44244 `decision.request` in the inbox of the party it is held on
 * (ledger 249(A)).
 *
 * Both 2026-09-22 control runs parked a seat behind a founder-held request
 * that surfaced nowhere a person looks. The question and the request's own
 * options are shown here, and the answer goes out through the same builder,
 * keyring and relay path the Mission decision queue uses.
 */
export function DecisionRequestInboxCard({
  request,
}: {
  request: InboxDecisionRequest;
}) {
  const identity = useIdentityQuery();
  const answering = useCodingSessionDecisionAnswer();
  const decision = inboxDecisionModel(request, identity.data?.pubkey ?? null);
  return (
    <div
      className="rounded-lg border border-border/60 bg-muted/20 px-3 py-2"
      data-testid="decision-request-card"
    >
      <p className="text-2xs font-medium text-muted-foreground">
        Decision requested · {decision.stateWord} · {decision.blocksWord}
      </p>
      <p
        className="mt-1 text-sm text-foreground"
        data-testid="decision-request-question"
      >
        {request.question}
      </p>
      <CodingSessionDecisionAnswerForm
        conditionMaxBytes={answering.capabilities?.conditionMaxBytes ?? 512}
        decision={decision}
        errorMessage={answering.errors[request.requestId] ?? null}
        onAnswer={(draft) => {
          void answering.answer({
            declaredOptions: request.options.length,
            channelRef: request.channelRef,
            sessionRef: request.sessionRef,
            genesisRef: request.genesisRef,
            draft,
          });
        }}
        pending={answering.pendingRequestId === request.requestId}
        publishedEventId={answering.published[request.requestId] ?? null}
        supportsCondition={
          answering.capabilities?.supportsDecisionAnswerCondition ?? null
        }
      />
      <p
        className="mt-1.5 break-all text-2xs text-muted-foreground"
        data-testid="decision-request-wake-disclosure"
      >
        {inboxDecisionWakeDisclosure(request)}
      </p>
    </div>
  );
}
