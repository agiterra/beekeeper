/**
 * The pure half of "Stop execution": what a stop request contains and how it
 * fans out into durable provider commands. Session closure is a separate
 * human-authored fact. Kept free of React and relay IO for node:test coverage.
 */
import { createCodingSessionLifecycleCommandId } from "./codingSessionLifecycleCommand";
import type { CodingSessionCommandTarget } from "./codingSessionCommand";

export type EndCodingSessionRequest = {
  /** The name the confirm dialog shows — the row's label. */
  label: string;
  channelId: string;
  stops: Array<{
    target: CodingSessionCommandTarget;
    providerAuthorityPubkey: string;
  }>;
};

/**
 * The structural slice of a session row this module needs. Any surface that
 * lists sessions (the project shelf, a future channel list) satisfies it
 * without this module importing that surface's types.
 */
export type EndableCodingSessionRow = {
  label: string;
  channelId: string;
  status: { kind: string };
  stopTargets: Array<{
    target: CodingSessionCommandTarget;
    providerAuthorityPubkey: string;
  }>;
};

/**
 * An umbrella row stands for every execution it collapsed; a bulk stop must
 * durably stop each non-ended execution, not just the representative.
 * Returns null when the row has nothing left to stop.
 */
export function buildEndCodingSessionStops(
  entry: EndableCodingSessionRow,
): EndCodingSessionRequest | null {
  if (entry.status.kind === "ended" || entry.stopTargets.length === 0) {
    return null;
  }
  return {
    label: entry.label,
    channelId: entry.channelId,
    stops: entry.stopTargets.map((stop) => ({ ...stop })),
  };
}

/**
 * Fan a request out into one stop publish per execution, each with a fresh
 * commandId. Failures don't abort the siblings, but the first failure is
 * reported so the person knows the bulk execution stop was not clean.
 */
export async function publishEndCodingSessionRequest(
  request: EndCodingSessionRequest,
  publishStop: (input: {
    channelId: string;
    commandId: string;
    target: CodingSessionCommandTarget;
    providerAuthorityPubkey: string;
  }) => Promise<unknown>,
): Promise<{ ok: boolean; errorMessage: string | null }> {
  const results = await Promise.allSettled(
    request.stops.map((stop) =>
      publishStop({
        channelId: request.channelId,
        commandId: createCodingSessionLifecycleCommandId(),
        target: stop.target,
        providerAuthorityPubkey: stop.providerAuthorityPubkey,
      }),
    ),
  );
  const failure = results.find(
    (result): result is PromiseRejectedResult => result.status === "rejected",
  );
  if (!failure) return { ok: true, errorMessage: null };
  const reason = failure.reason;
  return {
    ok: false,
    errorMessage:
      reason instanceof Error ? reason.message : "Failed to end the session.",
  };
}
