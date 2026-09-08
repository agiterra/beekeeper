import * as React from "react";

import {
  armCodingSessionDiscoveryOnConnect,
  type CodingSessionDiscoveryArmingClient,
} from "@/features/coding-sessions/lib/codingSessionDiscoveryArming";
import { relayClient as defaultRelayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_AUTHORITY_TRANSITION,
  KIND_SYSTEM_MESSAGE,
} from "@/shared/constants/kinds";

const AUTHORITY_CHANNELS_PER_SUBSCRIPTION = 128;
const AUTHORITY_RECEIPT_TYPE = "coding_session_authority_transition_accepted";
const UNIT_SEPARATOR = "\u0000";

export type RoleAuthorityLiveClient = {
  subscribeLive(
    filter: RelaySubscriptionFilter,
    onEvent: (event: RelayEvent) => void,
  ): Promise<() => void | Promise<void>>;
} & CodingSessionDiscoveryArmingClient;

export type RoleAuthorityLiveInput = {
  /** Active community identity; null keeps the watcher disabled. */
  relayUrl: string | null;
  channelIds: readonly string[];
  /** Roles is mounted and has a proof query worth refreshing. */
  enabled: boolean;
  /** Re-run the bounded authoritative read; a live event is only a hint. */
  onEvidence: () => void;
};

type ScopedError = {
  scopeIdentity: string;
  message: string | null;
};

type Watcher = {
  filter: RelaySubscriptionFilter;
  pending: boolean;
  dispose: (() => void | Promise<void>) | null;
};

function stableChannelIdentity(channelIds: readonly string[]): string {
  return [...new Set(channelIds.filter(Boolean))].sort().join(UNIT_SEPARATOR);
}

function authorityFilters(
  channelIds: readonly string[],
): RelaySubscriptionFilter[] {
  const filters: RelaySubscriptionFilter[] = [];
  for (
    let index = 0;
    index < channelIds.length;
    index += AUTHORITY_CHANNELS_PER_SUBSCRIPTION
  ) {
    filters.push({
      kinds: [KIND_CODING_SESSION_AUTHORITY_TRANSITION, KIND_SYSTEM_MESSAGE],
      "#h": channelIds.slice(
        index,
        index + AUTHORITY_CHANNELS_PER_SUBSCRIPTION,
      ),
      limit: 0,
    });
  }
  return filters;
}

function channelIsInScope(
  event: RelayEvent,
  channelIds: ReadonlySet<string>,
): boolean {
  return event.tags.some(
    (tag) => tag[0] === "h" && channelIds.has(tag[1] ?? ""),
  );
}

function isAuthorityReceipt(event: RelayEvent): boolean {
  if (event.kind !== KIND_SYSTEM_MESSAGE) return false;
  try {
    const content: unknown = JSON.parse(event.content);
    return (
      typeof content === "object" &&
      content !== null &&
      !Array.isArray(content) &&
      (content as Record<string, unknown>).type === AUTHORITY_RECEIPT_TYPE
    );
  } catch {
    return false;
  }
}

function isAuthorityEvidence(
  event: RelayEvent,
  channelIds: ReadonlySet<string>,
): boolean {
  if (!channelIsInScope(event, channelIds)) return false;
  return (
    event.kind === KIND_CODING_SESSION_AUTHORITY_TRANSITION ||
    isAuthorityReceipt(event)
  );
}

function errorText(error: unknown): string {
  if (error instanceof Error && error.message.trim() !== "") {
    return error.message.trim().replace(/\.+$/, "");
  }
  return "the relay did not establish the subscription";
}

function disposeQuietly(dispose: () => void | Promise<void>): void {
  try {
    void Promise.resolve(dispose()).catch(() => {});
  } catch {
    // Cleanup is best-effort; the effect is already fenced from callbacks.
  }
}

/**
 * Keep the mounted Roles proof query live for authority transitions.
 *
 * Subscription events are refresh hints only. The proof query performs the
 * bounded history read and signature/authority checks after this hook calls
 * `onEvidence`.
 */
export function useRoleAuthorityLive(
  input: RoleAuthorityLiveInput,
  client: RoleAuthorityLiveClient = defaultRelayClient,
): string | null {
  const channelIdentity = stableChannelIdentity(input.channelIds);
  const channelIds = React.useMemo(
    () => (channelIdentity === "" ? [] : channelIdentity.split(UNIT_SEPARATOR)),
    [channelIdentity],
  );
  const scopeIdentity = `${input.relayUrl ?? ""}|${channelIdentity}|${input.enabled}`;
  const [error, setError] = React.useState<ScopedError>({
    scopeIdentity,
    message: null,
  });
  const notifyEvidence = React.useEffectEvent(() => input.onEvidence());

  React.useEffect(() => {
    if (!input.enabled || input.relayUrl === null || channelIds.length === 0) {
      return;
    }

    let cancelled = false;
    let evidenceQueued = false;
    const scopedChannels = new Set(channelIds);
    const watchers: Watcher[] = authorityFilters(channelIds).map((filter) => ({
      filter,
      pending: false,
      dispose: null,
    }));

    const requestEvidence = () => {
      if (cancelled || evidenceQueued) return;
      evidenceQueued = true;
      queueMicrotask(() => {
        evidenceQueued = false;
        if (!cancelled) notifyEvidence();
      });
    };
    const onEvent = (event: RelayEvent) => {
      if (!cancelled && isAuthorityEvidence(event, scopedChannels)) {
        requestEvidence();
      }
    };
    const establish = async () => {
      const missing = watchers.filter(
        (watcher) => watcher.dispose === null && !watcher.pending,
      );
      if (missing.length === 0) {
        if (watchers.some((watcher) => watcher.pending)) return;
        if (watchers.some((watcher) => watcher.dispose !== null)) {
          requestEvidence();
        }
        return;
      }

      const failures: string[] = [];
      await Promise.all(
        missing.map(async (watcher) => {
          watcher.pending = true;
          try {
            const dispose = await client.subscribeLive(watcher.filter, onEvent);
            if (cancelled) disposeQuietly(dispose);
            else watcher.dispose = dispose;
          } catch (caught) {
            failures.push(errorText(caught));
          } finally {
            watcher.pending = false;
          }
        }),
      );
      if (cancelled) return;

      const stillMissing = watchers.some((watcher) => watcher.dispose === null);
      setError({
        scopeIdentity,
        message: stillMissing
          ? `Live role-authority evidence is unavailable: ${[
              ...new Set(failures),
            ].join("; ")}. Reconnect or reopen Roles to retry.`
          : null,
      });
      // Fence live delivery first, then backfill. This closes the gap between
      // the independently mounted proof query's first read and subscription.
      if (watchers.some((watcher) => watcher.dispose !== null)) {
        requestEvidence();
      }
    };

    setError({ scopeIdentity, message: null });
    void establish();
    const disarm = armCodingSessionDiscoveryOnConnect(client, () => {
      if (cancelled) return;
      void establish();
    });

    return () => {
      cancelled = true;
      disarm();
      for (const watcher of watchers) {
        if (watcher.dispose) disposeQuietly(watcher.dispose);
      }
    };
  }, [channelIds, client, input.enabled, input.relayUrl, scopeIdentity]);

  return error.scopeIdentity === scopeIdentity ? error.message : null;
}
