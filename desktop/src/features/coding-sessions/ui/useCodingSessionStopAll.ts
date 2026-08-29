import * as React from "react";

import { useEndCodingSessionDialog } from "@/features/coding-sessions/hooks/useEndCodingSessionDialog";
import type { CodingSessionReachabilityResolver } from "@/features/coding-sessions/hooks/useCodingSessionProviderReachability";
import {
  buildCodingSessionStopAll,
  type CodingSessionStopAllModel,
} from "@/features/coding-sessions/lib/codingSessionStopAllModel";
import type { CodingSessionActorNameResolver } from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import type { CodingSessionUmbrellaRecord } from "@/features/coding-sessions/lib/codingSessionTypes";
import { deriveCodingSessionWorkspaceStatus } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";

/**
 * "Stop all", wired to this umbrella's executions and this computer's relay.
 *
 * Item 87(e), found live 2026-08-28: a team launch seated a lead, the lead
 * hired two more seats, and the person who started it had no control anywhere
 * that stopped them — the per-execution stop lives in each execution's own
 * composer, which is exactly the surface you cannot reach when what you want
 * is "all of it, now".
 *
 * The decision itself (who may, and which executions are still live) is
 * `buildCodingSessionStopAll` and is proved there. This hook only derives each
 * execution's live status the way every other surface does and reuses the one
 * confirm-and-publish path — the same `publishCodingSessionStop` fan-out, the
 * same unanswered-provider toast, the same pending rows — so a bulk stop and a
 * single stop cannot drift into two behaviours.
 */
export function useCodingSessionStopAll(input: {
  channelId: string;
  currentUserPubkey: string | null;
  resolveActorName: CodingSessionActorNameResolver;
  resolveReachability: CodingSessionReachabilityResolver;
  umbrella: CodingSessionUmbrellaRecord;
}): {
  /** What the header should offer — or why it should offer nothing. */
  model: CodingSessionStopAllModel;
  /** Opens the confirm. A no-op when the model is `hidden`. */
  stopAll: () => void;
  dialog: React.ReactNode;
} {
  const {
    channelId,
    currentUserPubkey,
    resolveActorName,
    resolveReachability,
    umbrella,
  } = input;
  const model = React.useMemo(
    () =>
      buildCodingSessionStopAll({
        channelId,
        founderPubkey: umbrella.founderPubkey,
        currentUserPubkey,
        executions: umbrella.executions.map((execution) => ({
          // The seat's own name when it has one, its role when nothing
          // resolves it, and the execution's title for a person's own
          // execution. An agent's work must never present itself as a
          // person's, so the seat is never labelled by the session.
          label: codingSessionStopAllLabel(
            execution.activeGeneration,
            resolveActorName,
          ),
          status: deriveCodingSessionWorkspaceStatus(
            execution.activeGeneration.transcript,
            execution.activeGeneration.status,
            execution.activeGeneration.statusAt,
            resolveReachability(execution.activeGeneration.commandTarget),
          ),
          target: execution.activeGeneration.commandTarget,
          providerAuthorityPubkey:
            execution.activeGeneration.providerAuthorityPubkey,
        })),
      }),
    [
      channelId,
      currentUserPubkey,
      resolveActorName,
      resolveReachability,
      umbrella.executions,
      umbrella.founderPubkey,
    ],
  );
  const { dialog, requestEnd } = useEndCodingSessionDialog();

  const stopAll = React.useCallback(() => {
    if (model.kind !== "available") return;
    requestEnd(model.request);
  }, [model, requestEnd]);
  return { dialog, model, stopAll };
}

/** How one execution is named in the confirm. */
function codingSessionStopAllLabel(
  record: CodingSessionUmbrellaRecord["executions"][number]["activeGeneration"],
  resolveActorName: CodingSessionActorNameResolver,
): string {
  if (!record.agentRef) return record.title;
  return resolveActorName(record.agentRef) ?? record.role ?? record.title;
}
