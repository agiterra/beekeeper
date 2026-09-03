/**
 * Optimistic sidebar state for coding-session lifecycle commands.
 *
 * Creating or ending a session publishes a signed kind-44221 command, but the
 * sidebar's session shelf is fact-driven: rows appear/settle only when the
 * provider answers with 44223 metadata. That gap used to read as "nothing
 * happened". This module records the user's own published commands and lets
 * the shelf overlay them — a synthesized "starting" row for a create, an
 * immediate "Ended" override for a stop — until the provider's signed facts
 * arrive and the pending record is consumed.
 *
 * The store is deliberately not persistence: it survives neither reload nor
 * community switch (reset via `resetPendingCodingSessionLifecycle`), and every
 * record expires after {@link PENDING_CODING_SESSION_LIFECYCLE_TTL_MS} so a
 * provider that never answers cannot leave a ghost row.
 */
import * as React from "react";

import { buildCodingSessionTargetKey } from "./codingSessionCommand";
import type {
  CodingSessionCatalogRecord,
  CodingSessionWorkspaceStatus,
} from "./codingSessionTypes";

export type PendingCodingSessionCreate = {
  kind: "create";
  channelId: string;
  commandId: string;
  /** Umbrella ref signed into the create; joins the provider's 44223 echo. */
  sessionRef: string | null;
  title: string | null;
  projectRef: string | null;
  providerAuthorityPubkey: string;
  /** Whether the create carried an initial turn (row shows Working vs Idle). */
  hasInitialTurn: boolean;
  recordedAt: number;
};

export type PendingCodingSessionStop = {
  kind: "stop";
  channelId: string;
  /** `buildCodingSessionTargetKey` of the execution being stopped. */
  targetKey: string;
  providerAuthorityPubkey: string;
  recordedAt: number;
};

export type PendingCodingSessionLifecycle =
  | PendingCodingSessionCreate
  | PendingCodingSessionStop;

/** A pending command the provider never acknowledged stops overlaying. */
export const PENDING_CODING_SESSION_LIFECYCLE_TTL_MS = 3 * 60_000;

export function pendingCodingSessionLifecycleKey(
  pending: PendingCodingSessionLifecycle,
): string {
  return pending.kind === "create"
    ? `create:${pending.channelId}:${pending.commandId}`
    : `stop:${pending.channelId}:${pending.targetKey}:${pending.providerAuthorityPubkey}`;
}

let pendingEntries: readonly PendingCodingSessionLifecycle[] = [];
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

export function recordPendingCodingSessionLifecycle(
  pending: PendingCodingSessionLifecycle,
): void {
  const key = pendingCodingSessionLifecycleKey(pending);
  pendingEntries = [
    ...pendingEntries.filter(
      (entry) => pendingCodingSessionLifecycleKey(entry) !== key,
    ),
    pending,
  ];
  notify();
}

/** Drop the given records (consumed by arriving facts, or expired). */
export function clearPendingCodingSessionLifecycle(
  keys: readonly string[],
): void {
  if (keys.length === 0) return;
  const drop = new Set(keys);
  const next = pendingEntries.filter(
    (entry) => !drop.has(pendingCodingSessionLifecycleKey(entry)),
  );
  if (next.length === pendingEntries.length) return;
  pendingEntries = next;
  notify();
}

/** Drop records past their TTL. Consumers schedule this off the earliest
 * expiry so an unanswered command's ghost row disappears even when nothing
 * else re-renders. */
export function sweepExpiredPendingCodingSessionLifecycle(now: number): void {
  clearPendingCodingSessionLifecycle(
    pendingEntries
      .filter(
        (entry) =>
          now - entry.recordedAt > PENDING_CODING_SESSION_LIFECYCLE_TTL_MS,
      )
      .map(pendingCodingSessionLifecycleKey),
  );
}

/** Community switch teardown — see `resetCommunityState()`. */
export function resetPendingCodingSessionLifecycle(): void {
  if (pendingEntries.length === 0) return;
  pendingEntries = [];
  notify();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function getSnapshot(): readonly PendingCodingSessionLifecycle[] {
  return pendingEntries;
}

export function usePendingCodingSessionLifecycle(): readonly PendingCodingSessionLifecycle[] {
  return React.useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}

/**
 * The structural slice of a shelf entry the overlay needs. Matches
 * `ProjectCodingSessionShelfEntry` (projects-container) without importing it —
 * lib must not depend on a feature module.
 */
export type PendingOverlayShelfEntry = {
  placement: "project" | "unassigned";
  projectId: string | null;
  placedBy: "project-ref" | "channel" | null;
  channelId: string;
  generationId: string;
  label: string;
  sourceChannelLabel: string | null;
  runtimeLabel: string | null;
  runtimeLabels: string[];
  executionCount: number;
  status: CodingSessionWorkspaceStatus;
  stopTargets: Array<{
    target: NonNullable<CodingSessionCatalogRecord["commandTarget"]>;
    providerAuthorityPubkey: string;
  }>;
  session: CodingSessionCatalogRecord;
  /** True only on synthesized rows for a not-yet-acknowledged create. */
  pending?: boolean;
};

export type ApplyPendingCodingSessionLifecycleResult<
  Entry extends PendingOverlayShelfEntry,
> = {
  entries: Array<Entry | PendingOverlayShelfEntry>;
  /** Keys whose underlying fact arrived (or that expired) — clear these. */
  consumedKeys: string[];
};

/**
 * The lifecycle-resolution slice the overlay consumes: enough to know whether
 * the create's 44224 receipt arrived and, when it did, which exact session
 * target it minted. Structural on purpose — the caller passes the catalog's
 * `lifecycleFor` closure.
 */
export type PendingCreateLifecycleResolver = (
  channelId: string,
  commandId: string,
  providerAuthorityPubkey: string,
) =>
  | { state: string; target?: CodingSessionCatalogRecord["commandTarget"] }
  | null
  | undefined;

/**
 * Merge pending lifecycle commands into resolved shelf entries.
 *
 * Pure: placement is decided by the caller (it owns the placement index), so
 * a pending create arrives here with its already-resolved projectId. A create
 * synthesizes a row until its receipt-confirmed session target (or, as a
 * backstop, its sessionRef echo) appears as a real entry; a stop forces the
 * matching row to Ended unless the row already ended.
 */
export function applyPendingCodingSessionLifecycle<
  Entry extends PendingOverlayShelfEntry,
>(
  entries: readonly Entry[],
  pending: readonly PendingCodingSessionLifecycle[],
  resolvePlacement: (
    projectRef: string | null,
    channelId: string,
  ) => {
    projectId: string | null;
    placedBy: "project-ref" | "channel" | null;
  },
  channelLabels: ReadonlyMap<string, string>,
  now: number,
  lifecycleFor?: PendingCreateLifecycleResolver,
): ApplyPendingCodingSessionLifecycleResult<Entry> {
  const consumedKeys: string[] = [];
  const stopOverrides = new Map<string, PendingCodingSessionStop>();
  const synthesized: PendingOverlayShelfEntry[] = [];

  for (const record of pending) {
    const key = pendingCodingSessionLifecycleKey(record);
    if (now - record.recordedAt > PENDING_CODING_SESSION_LIFECYCLE_TTL_MS) {
      consumedKeys.push(key);
      continue;
    }
    if (record.kind === "create") {
      // The 44224 receipt is the authoritative consumption signal: it is
      // addressed by exactly (channel, commandId), arrives before metadata,
      // and carries the minted session target. The metadata sessionRef echo
      // stays as a backstop — it is the only evidence that survives in the
      // persisted shelf cache — but it is the LAST fact to arrive, so it can
      // never be the primary signal (a transcript-only real row has
      // sessionRef null for an unbounded window).
      const resolution =
        lifecycleFor?.(
          record.channelId,
          record.commandId,
          record.providerAuthorityPubkey,
        ) ?? null;
      if (resolution?.state === "failed" || resolution?.state === "conflict") {
        // Definitive outcome: the create screen owns the error surface; a
        // spinner row would just contradict it.
        consumedKeys.push(key);
        continue;
      }
      const receiptTargetKey = resolution?.target
        ? buildCodingSessionTargetKey(resolution.target)
        : null;
      const acknowledged = entries.some(
        (entry) =>
          entry.channelId === record.channelId &&
          ((receiptTargetKey !== null &&
            entry.session.commandTarget !== null &&
            buildCodingSessionTargetKey(entry.session.commandTarget) ===
              receiptTargetKey) ||
            (record.sessionRef !== null &&
              entry.session.sessionRef === record.sessionRef)),
      );
      if (acknowledged) {
        consumedKeys.push(key);
        continue;
      }
      synthesized.push(
        synthesizePendingEntry(record, resolvePlacement, channelLabels),
      );
      continue;
    }
    const target = entries.find(
      (entry) =>
        entry.channelId === record.channelId &&
        entry.stopTargets.some(
          (stop) =>
            stop.providerAuthorityPubkey === record.providerAuthorityPubkey &&
            buildCodingSessionTargetKey(stop.target) === record.targetKey,
        ),
    );
    // A stop overlays whichever live row still advertises its exact target.
    // Once the provider's `stopped` metadata lands, the row settles and drops
    // its stopTargets, so the override simply stops matching — the record is
    // then inert and expires by TTL. Fact-based consumption is deliberately
    // not attempted: a transient empty snapshot (channel-set change refetch)
    // must not eat a stop whose row will reappear un-settled moments later.
    if (target) {
      stopOverrides.set(`${target.channelId} ${target.generationId}`, record);
    }
  }

  const overlaid = entries.map((entry) => {
    const override = stopOverrides.get(
      `${entry.channelId} ${entry.generationId}`,
    );
    if (!override || entry.status.kind === "ended") return entry;
    return {
      ...entry,
      status: { kind: "ended", label: "Ended" } as const,
      stopTargets: [],
    };
  });

  return { entries: [...synthesized, ...overlaid], consumedKeys };
}

function synthesizePendingEntry(
  record: PendingCodingSessionCreate,
  resolvePlacement: (
    projectRef: string | null,
    channelId: string,
  ) => {
    projectId: string | null;
    placedBy: "project-ref" | "channel" | null;
  },
  channelLabels: ReadonlyMap<string, string>,
): PendingOverlayShelfEntry {
  const placement = resolvePlacement(record.projectRef, record.channelId);
  const label =
    record.title && record.title.trim().length > 0
      ? record.title.trim()
      : "Coding session";
  const status: CodingSessionWorkspaceStatus = record.hasInitialTurn
    ? { kind: "working", label: "Working" }
    : { kind: "idle", label: "Idle" };
  const session: CodingSessionCatalogRecord = {
    generationId: `pending:${record.commandId}`,
    label,
    title: record.title?.trim() ?? "",
    providerAuthorityPubkey: record.providerAuthorityPubkey,
    metadataAuthorityPubkey: null,
    lastEventAt: new Date(record.recordedAt).toISOString(),
    status: record.hasInitialTurn ? "running" : "idle",
    statusAt: null,
    statusEventId: null,
    transcript: [],
    conflictCount: 0,
    commandTarget: null,
    projectRef: record.projectRef,
    repoRef: null,
    sessionRef: record.sessionRef,
    provider: null,
    runtime: null,
    model: null,
    // A pending create has published no metadata yet, so nothing is known
    // about its seat — including whether it has one.
    agentRef: null,
    role: null,
    turnBudget: null,
    // A pending create has not been routed by anything this client can see:
    // the seat's routing arrives with the provider's 44223, not before it.
    routing: null,
    capabilities: null,
    // Same reasoning as `routing`: a pending create has published no 44223
    // yet, so nothing is known about which `bee` its seat will run.
    beeStamp: null,
  };
  return {
    placement: placement.projectId ? "project" : "unassigned",
    projectId: placement.projectId,
    placedBy: placement.placedBy,
    channelId: record.channelId,
    generationId: session.generationId,
    label,
    sourceChannelLabel: channelLabels.get(record.channelId)?.trim() || null,
    runtimeLabel: null,
    runtimeLabels: [],
    executionCount: 1,
    status,
    stopTargets: [],
    session,
    pending: true,
  };
}
