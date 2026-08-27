/**
 * The gate a crew launch waits on: one seat's 44224 create receipt.
 *
 * The single-session create flow reads receipts through a React hook, which is
 * the right shape when exactly one command is in flight. A crew launch runs N
 * creates in order, each gated on the last, so the wait has to be an awaitable
 * — otherwise "receipt-gated" collapses into "published all of them and hoped".
 *
 * The read is exactly as narrow as the hook's: a store pinned to the one
 * provider the create named (`buildPinnedCodingSessionIngressAuthority`), the
 * same classifier, the same lifecycle resolution. A receipt from anyone else
 * is dropped, not believed.
 */
import { relayClient as defaultRelayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_CODING_SESSION_LIFECYCLE_RECEIPT } from "@/shared/constants/kinds";
import type { CodingSessionCommandTarget } from "./codingSessionCommand";
import { buildPinnedCodingSessionIngressAuthority } from "./codingSessionIngressAuthority";
import {
  TrustedCodingSessionIngressStore,
  type CodingSessionLifecycleResolution,
} from "./codingSessionTrustedIngress";

/** How long a seat's receipt may take before the launch calls it stuck. */
export const CODING_SESSION_CREW_RECEIPT_TIMEOUT_MS = 120_000;

export type CodingSessionCrewReceiptClient = {
  fetchEvents(filter: RelaySubscriptionFilter): Promise<RelayEvent[]>;
  subscribeLive(
    filter: RelaySubscriptionFilter,
    onEvent: (event: RelayEvent) => void,
  ): Promise<() => void>;
};

/**
 * The target a receipt minted, whatever the provider went on to say about
 * metadata.
 *
 * Wider than `establishedCodingSessionTarget` on purpose: the launch's gate is
 * *the seat exists*, and a seat awaiting its metadata exists. Narrowing to the
 * metadata-complete states would make a healthy launch hang on a fact the next
 * step does not need.
 */
export function codingSessionCrewReceiptTarget(
  lifecycle: CodingSessionLifecycleResolution,
): CodingSessionCommandTarget | null {
  return "target" in lifecycle ? lifecycle.target : null;
}

/** The receipt filter for one command: one kind, one channel, one provider. */
export function buildCodingSessionCrewReceiptFilter(input: {
  channelId: string;
  providerAuthorityPubkey: string;
}): RelaySubscriptionFilter {
  return {
    kinds: [KIND_CODING_SESSION_LIFECYCLE_RECEIPT],
    "#h": [input.channelId],
    authors: [input.providerAuthorityPubkey],
    limit: 200,
  };
}

/**
 * Wait for the provider's answer to one create, and hand back the target it
 * minted.
 *
 * Rejects — never resolves ambiguously — when the provider refuses the create,
 * when two receipts disagree, or when nothing arrives inside the timeout. The
 * caller turns that into a named failed step; it must not proceed to the next
 * seat on any of them.
 */
export async function awaitCodingSessionCreateReceipt(
  input: {
    channelId: string;
    commandId: string;
    providerAuthorityPubkey: string;
    timeoutMs?: number;
  },
  dependencies: {
    client?: CodingSessionCrewReceiptClient;
    setTimer?: (callback: () => void, ms: number) => unknown;
    clearTimer?: (handle: unknown) => void;
  } = {},
): Promise<CodingSessionCommandTarget> {
  const client = dependencies.client ?? defaultRelayClient;
  const setTimer =
    dependencies.setTimer ??
    ((callback, ms) => globalThis.setTimeout(callback, ms));
  const clearTimer =
    dependencies.clearTimer ??
    ((handle) => globalThis.clearTimeout(handle as never));
  const authority = buildPinnedCodingSessionIngressAuthority(
    input.providerAuthorityPubkey,
  );
  const store = new TrustedCodingSessionIngressStore();
  const filter = buildCodingSessionCrewReceiptFilter(input);
  const channelIds = [input.channelId];

  return new Promise<CodingSessionCommandTarget>((resolve, reject) => {
    let settled = false;
    let unsubscribe: (() => void) | null = null;
    let timer: unknown = null;
    const finish = (action: () => void) => {
      if (settled) return;
      settled = true;
      if (timer !== null) clearTimer(timer);
      unsubscribe?.();
      action();
    };
    const consider = (events: readonly RelayEvent[]) => {
      store.ingestRelayEvents(events, channelIds, authority);
      const lifecycle = store.resolveLifecycle(
        input.channelId,
        input.commandId,
        input.providerAuthorityPubkey,
      );
      if (lifecycle.state === "failed") {
        finish(() =>
          reject(
            new Error(
              `${lifecycle.error.code}: ${lifecycle.error.message}`.trim(),
            ),
          ),
        );
        return;
      }
      if (lifecycle.state === "conflict") {
        finish(() =>
          reject(
            new Error(
              "Two different receipts answered this seat's create — the launch cannot tell which execution it made.",
            ),
          ),
        );
        return;
      }
      const target = codingSessionCrewReceiptTarget(lifecycle);
      if (target) finish(() => resolve(target));
    };

    timer = setTimer(() => {
      finish(() =>
        reject(
          new Error(
            "The provider did not answer within the wait — the seat may still be created.",
          ),
        ),
      );
    }, input.timeoutMs ?? CODING_SESSION_CREW_RECEIPT_TIMEOUT_MS);

    // Live first, then backfill: a receipt that lands between the two reads
    // must not fall into the gap between them.
    client
      .subscribeLive(filter, (event) => consider([event]))
      .then((stop) => {
        if (settled) {
          stop();
          return;
        }
        unsubscribe = stop;
        return client.fetchEvents(filter).then(consider);
      })
      .catch((error: unknown) => {
        finish(() =>
          reject(
            error instanceof Error
              ? error
              : new Error("Could not read the seat's create receipt."),
          ),
        );
      });
  });
}
