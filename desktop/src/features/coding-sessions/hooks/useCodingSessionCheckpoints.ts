/**
 * Reads one session's turn checkpoints (kind 44231) and folds them (SV-28).
 *
 * The separate-REQ model `useCodingSessionHandover` uses: one bounded,
 * channel-scoped read, then a live REQ `since` the moment it opened whose
 * events are merged into the same cached result — no polling, no refetch per
 * event. `authors` is the set of keys that sign this session's 44225 items,
 * so a stranger's events cannot spend the bound; the fold still re-checks
 * every signer against its own target, because the relay checks structure
 * only.
 *
 * `retry: false` on purpose: a refused read is something to say, and a retry
 * loop would replace it with a spinner.
 */
import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { relayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_CODING_SESSION_CHECKPOINT } from "@/shared/constants/kinds";
import { buildCodingSessionTargetKey } from "../lib/codingSessionCommand";
import {
  type CodingSessionCheckpointClassification,
  type CodingSessionCheckpointFold,
  type CodingSessionCheckpointScope,
  classifyCodingSessionCheckpointEvent,
  codingSessionCheckpointScopeKey,
  EMPTY_CODING_SESSION_CHECKPOINT_FOLD,
  foldCodingSessionCheckpoints,
} from "../lib/codingSessionCheckpoints";
import type { CodingSessionUmbrellaRecord } from "../lib/codingSessionTypes";

/** How many checkpoints one read retains. One per turn, so generous. */
export const CODING_SESSION_CHECKPOINT_HISTORY_LIMIT = 512;

/** One generation this view shows, and who signs its transcript. */
export type CodingSessionCheckpointGeneration = {
  generationId: string;
  targetKey: string;
  /** The 44225 signer; only a generation with transcript items has one. */
  signerPubkey: string;
};

export type CodingSessionCheckpointsRead = {
  fold: CodingSessionCheckpointFold;
  /** `generationId` → fold scope key, for the turn and Diff lookups. */
  scopeByGeneration: ReadonlyMap<string, string>;
  /** Generation facts by `generationId`, for the native diff request. */
  generations: ReadonlyMap<string, CodingSessionCheckpointGeneration>;
  /** The read came back at its own ceiling: older checkpoints may be missing. */
  capped: boolean;
  isLoading: boolean;
  errorMessage: string | null;
};

type CheckpointClient = {
  fetchEventsBatch(filters: RelaySubscriptionFilter[]): Promise<RelayEvent[]>;
};

type CheckpointData = { events: readonly RelayEvent[]; capped: boolean };

/**
 * The generations whose checkpoints this view may trust: every generation of
 * every execution that has transcript items, with the key that signed them.
 */
export function codingSessionCheckpointGenerations(
  umbrella: CodingSessionUmbrellaRecord | null,
): CodingSessionCheckpointGeneration[] {
  if (!umbrella) return [];
  const generations: CodingSessionCheckpointGeneration[] = [];
  for (const execution of umbrella.executions) {
    for (const record of [
      ...execution.priorGenerations,
      execution.activeGeneration,
    ]) {
      const signer = record.providerAuthorityPubkey;
      if (!record.commandTarget || !signer || record.transcript.length === 0) {
        continue;
      }
      generations.push({
        generationId: record.generationId,
        targetKey: buildCodingSessionTargetKey(record.commandTarget),
        signerPubkey: signer,
      });
    }
  }
  return generations;
}

/** The bounded history filter, with explicit `kinds`, `#h` and `authors`. */
export function buildCodingSessionCheckpointFilter(
  channelId: string,
  authors: readonly string[],
  limit: number = CODING_SESSION_CHECKPOINT_HISTORY_LIMIT,
): RelaySubscriptionFilter {
  return {
    kinds: [KIND_CODING_SESSION_CHECKPOINT],
    "#h": [channelId],
    authors: [...authors],
    limit,
  };
}

/** The live filter: new checkpoints only, from `since`. */
export function buildCodingSessionCheckpointLiveFilter(
  channelId: string,
  authors: readonly string[],
  since: number,
): RelaySubscriptionFilter {
  return {
    kinds: [KIND_CODING_SESSION_CHECKPOINT],
    "#h": [channelId],
    authors: [...authors],
    since,
    limit: 0,
  };
}

/** One bounded read. Exported without React so a test can drive it. */
export async function readCodingSessionCheckpoints(input: {
  channelId: string;
  authors: readonly string[];
  client: CheckpointClient;
}): Promise<CheckpointData> {
  const events = await input.client.fetchEventsBatch([
    buildCodingSessionCheckpointFilter(input.channelId, input.authors),
  ]);
  const checkpoints = events.filter(
    (event) => event.kind === KIND_CODING_SESSION_CHECKPOINT,
  );
  return {
    events: checkpoints,
    capped: checkpoints.length >= CODING_SESSION_CHECKPOINT_HISTORY_LIMIT,
  };
}

/** Merge one live event into a read, by id. Unchanged data stays identical. */
export function mergeCodingSessionCheckpointEvent(
  data: CheckpointData | undefined,
  event: RelayEvent,
): CheckpointData | undefined {
  if (!data || event.kind !== KIND_CODING_SESSION_CHECKPOINT) return data;
  if (data.events.some((held) => held.id === event.id)) return data;
  return { events: [...data.events, event], capped: data.capped };
}

/**
 * A classifier that remembers its verdict per event id, so a re-fold after a
 * live event hands every earlier checkpoint back as the same object and the
 * turns reading it keep their identity (SV-100). The verdict depends on the
 * scope, so the memory is dropped whenever the scope changes.
 */
export function createCodingSessionCheckpointClassifier(): (
  event: RelayEvent,
  scope: CodingSessionCheckpointScope,
) => CodingSessionCheckpointClassification {
  let heldScope: CodingSessionCheckpointScope | null = null;
  const verdicts = new Map<string, CodingSessionCheckpointClassification>();
  return (event, scope) => {
    if (scope !== heldScope) {
      verdicts.clear();
      heldScope = scope;
    }
    const held = verdicts.get(event.id);
    if (held) return held;
    const verdict = classifyCodingSessionCheckpointEvent(event, scope);
    verdicts.set(event.id, verdict);
    return verdict;
  };
}

/** Read one session's checkpoints. A null channel or umbrella reads nothing. */
export function useCodingSessionCheckpoints(
  channelId: string | null,
  umbrella: CodingSessionUmbrellaRecord | null,
  options: { client?: CheckpointClient } = {},
): CodingSessionCheckpointsRead {
  const queryClient = useQueryClient();
  const client = options.client ?? relayClient;
  // The umbrella is a fresh object on every transcript item; everything here
  // keys on a string signature of what matters, so nothing restarts.
  const signature = JSON.stringify(
    codingSessionCheckpointGenerations(umbrella)
      .map((entry) => [entry.generationId, entry.targetKey, entry.signerPubkey])
      .sort((left, right) => left.join("\n").localeCompare(right.join("\n"))),
  );
  const derived = React.useMemo(() => {
    const generations = new Map<string, CodingSessionCheckpointGeneration>();
    const scopeByGeneration = new Map<string, string>();
    const signersByTargetKey = new Map<string, Set<string>>();
    const rows = JSON.parse(signature) as [string, string, string][];
    for (const [generationId, targetKey, signerPubkey] of rows) {
      generations.set(generationId, { generationId, targetKey, signerPubkey });
      scopeByGeneration.set(
        generationId,
        codingSessionCheckpointScopeKey(signerPubkey, targetKey),
      );
      const signers = signersByTargetKey.get(targetKey) ?? new Set<string>();
      signers.add(signerPubkey);
      signersByTargetKey.set(targetKey, signers);
    }
    const authors = [
      ...new Set([...generations.values()].map((entry) => entry.signerPubkey)),
    ].sort();
    return { authors, generations, scopeByGeneration, signersByTargetKey };
  }, [signature]);
  const authorsKey = derived.authors.join(",");
  const enabled = channelId !== null && derived.authors.length > 0;
  const key = React.useMemo(
    () => ["coding-session-checkpoints", channelId, authorsKey] as const,
    [authorsKey, channelId],
  );
  const query = useQuery({
    queryKey: key,
    enabled,
    retry: false,
    queryFn: () =>
      readCodingSessionCheckpoints({
        channelId: channelId ?? "",
        authors: derived.authors,
        client,
      }),
  });

  React.useEffect(() => {
    if (!enabled || channelId === null) return;
    let disposed = false;
    let leave: (() => Promise<void>) | null = null;
    const since = Math.floor(Date.now() / 1_000);
    const authors = authorsKey.split(",");
    void (async () => {
      try {
        const unsubscribe = await relayClient.subscribeLiveMany(
          [buildCodingSessionCheckpointLiveFilter(channelId, authors, since)],
          (event) => {
            queryClient.setQueryData<CheckpointData>(key, (data) =>
              mergeCodingSessionCheckpointEvent(data, event),
            );
          },
        );
        if (disposed) {
          void unsubscribe();
          return;
        }
        leave = unsubscribe;
      } catch {
        // A refused live REQ leaves the one-shot read as the only source.
      }
    })();
    return () => {
      disposed = true;
      void leave?.();
    };
  }, [authorsKey, channelId, enabled, key, queryClient]);

  const scope = React.useMemo<CodingSessionCheckpointScope>(
    () => ({
      channelId: channelId ?? "",
      signersByTargetKey: derived.signersByTargetKey,
    }),
    [channelId, derived.signersByTargetKey],
  );
  const [classify] = React.useState(createCodingSessionCheckpointClassifier);
  const data = query.data;
  const fold = React.useMemo(
    () =>
      data
        ? foldCodingSessionCheckpoints(data.events, scope, classify)
        : EMPTY_CODING_SESSION_CHECKPOINT_FOLD,
    [classify, data, scope],
  );
  const error = query.error;
  const isLoading = enabled && query.isLoading;
  return React.useMemo(
    () => ({
      fold,
      scopeByGeneration: derived.scopeByGeneration,
      generations: derived.generations,
      capped: data?.capped ?? false,
      isLoading,
      errorMessage:
        error instanceof Error ? error.message : error ? String(error) : null,
    }),
    [data?.capped, derived, error, fold, isLoading],
  );
}
