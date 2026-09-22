import * as React from "react";

import { useCodingSessionDecisionAnswer } from "@/features/coding-sessions/hooks/useCodingSessionDecisionAnswer";
import { publishCodingSessionDecisionAnswer } from "@/features/coding-sessions/lib/codingSessionTeamTransactionPublish";
import { CodingSessionDecisionAnswerForm } from "@/features/coding-sessions/ui/CodingSessionDecisionAnswerForm";
import { useCodingSessionCatalog } from "@/features/coding-sessions/useCodingSessionCatalog";
import { useIdentityQuery } from "@/shared/api/hooks";
import { truncatePubkey } from "@/shared/lib/pubkey";

import {
  type DecisionAnswerWake,
  resolveAskerTarget,
  wakeAskerForAnswer,
} from "../lib/decisionAnswerWake";
import {
  inboxDecisionModel,
  type InboxDecisionRequest,
} from "../lib/decisionRequestInbox";

/**
 * A kind:44244 `decision.request` in the inbox of the party it is held on
 * (ledger 249(A)).
 *
 * Both 2026-09-22 control runs parked a seat behind a founder-held request
 * that surfaced nowhere a person looks. The question and the request's own
 * options are shown here; the answer goes out through the Mission queue's
 * builder, keyring and relay path, and then — as `bee sessions decide
 * answer` does by default — one kind:44220 wakes the asker's execution.
 */
export function DecisionRequestInboxCard({
  request,
}: {
  request: InboxDecisionRequest;
}) {
  const identity = useIdentityQuery();
  const catalog = useCodingSessionCatalog(request.channelRef);
  const asker = React.useMemo(
    () =>
      resolveAskerTarget(
        catalog.entries,
        request.sessionRef,
        request.askerPubkey,
      ),
    [catalog.entries, request.askerPubkey, request.sessionRef],
  );
  const askerRef = React.useRef(asker);
  askerRef.current = asker;
  const [wake, setWake] = React.useState<DecisionAnswerWake | null>(null);
  const deps = React.useMemo(
    () => ({
      publish: async (
        input: Parameters<typeof publishCodingSessionDecisionAnswer>[0],
      ) => {
        const published = await publishCodingSessionDecisionAnswer({
          ...input,
          deps: undefined,
        });
        setWake(
          await wakeAskerForAnswer({
            channelRef: input.channelRef,
            answerEventId: published.eventId,
            asker: askerRef.current,
            askerLabel: truncatePubkey(request.askerPubkey),
          }),
        );
        return published;
      },
    }),
    [request.askerPubkey],
  );
  const answering = useCodingSessionDecisionAnswer(deps);
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
        // Whom to wake is read from the channel's sessions; answering before
        // that read settles would report "no seat" for a seat that exists.
        pending={
          catalog.isLoading || answering.pendingRequestId === request.requestId
        }
        publishedEventId={answering.published[request.requestId] ?? null}
        supportsCondition={
          answering.capabilities?.supportsDecisionAnswerCondition ?? null
        }
      />
      {catalog.isLoading ? (
        <p
          className="mt-1.5 text-2xs text-muted-foreground"
          data-testid="decision-request-finding-asker"
        >
          Finding the asker's seat, so the answer can wake it…
        </p>
      ) : null}
      {wake ? (
        <p
          className={
            wake.status === "sent"
              ? "mt-1.5 text-2xs text-muted-foreground"
              : "mt-1.5 text-2xs text-destructive"
          }
          data-testid={`decision-request-wake-${wake.status}`}
          role="status"
        >
          {wake.status === "sent"
            ? `Woke ${wake.seat} · ${wake.eventId.slice(0, 8)} · its receipts show when the turn starts`
            : wake.message}
        </p>
      ) : null}
    </div>
  );
}
