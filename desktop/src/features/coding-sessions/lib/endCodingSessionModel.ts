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
  /**
   * True when coordination proves nothing is answering for these executions.
   *
   * A stop published at a dead provider is accepted by the relay and sits
   * there: Brian's three clicks drained two hours later, when the app that
   * owned the execution came back (§2 item 42). The command is still worth
   * publishing — that is what made those stops eventually run — but the person
   * pressing it has to be told it is a request, not an effect.
   */
  providerUnanswered?: boolean;
  /**
   * Confirm copy for a request that is not "stop this one execution".
   *
   * The bulk control in the umbrella header stops every live seat at once, and
   * a dialog titled "Stop this execution?" over three of them would understate
   * what the button does. Absent keeps the single-execution wording, which is
   * what every existing caller wants.
   */
  confirm?: { title: string; description: string; action: string };
};

/**
 * What the confirm dialog says, as a pure function of the request.
 *
 * Two facts the old copy left out, both bought with live confusion (§2 item
 * 42): a stopped execution is never resumable — the composer offers Reconnect
 * for `disconnected` only — and a stop aimed at a provider that is not
 * answering is a queued request rather than an effect.
 */
export function endCodingSessionDialogDescription(
  request: EndCodingSessionRequest | null,
): string {
  if (request === null) return "";
  const base =
    `The provider execution for "${request.label}" will be stopped for ` +
    "everyone, and a stopped execution cannot be resumed — to carry the work " +
    "on, add a provider to the session. The durable session and its " +
    "transcript stay open; close the session separately when the work is " +
    "finished.";
  return request.providerUnanswered
    ? `${base} No provider is answering for it right now, so this is a ` +
        "request: it stays on the relay and runs whenever one returns."
    : base;
}

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
