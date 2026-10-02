/**
 * Writing a project's artifact pins (NIP-AR, kind 44251).
 *
 * The shape is `agentsRepoMutations.ts`'s: read the cache to stamp
 * `created_at` past the latest op on the same target, sign, publish, append
 * optimistically, invalidate to reconcile. What differs is that a pin needs no
 * head check — there is no text anyone could lose, and the fold resolves two
 * concurrent writes by field, so the only thing a stale read costs is a rank
 * placed between the neighbours it saw.
 */
import * as React from "react";
import { type QueryClient, useQueryClient } from "@tanstack/react-query";

import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_PROJECT_ARTIFACT_PIN_OP } from "@/shared/constants/kinds";
import { rankBetween } from "@/features/project-todos/lib/fractionalRank";

import type { PinRow } from "./artifactPinFold";
import {
  type PinOp,
  type PinTargetKind,
  encodePinOpContent,
  pinOpTags,
  pinTargetError,
} from "./artifactPinOp";
import {
  type ArtifactPinsRead,
  RELAY_TIMESTAMP_DRIFT_SECS,
  artifactPinsQueryKey,
  pinsReadFromEvents,
} from "./artifactPinQueries";

const MAX_FUTURE_SKEW_SECS = RELAY_TIMESTAMP_DRIFT_SECS - 10;

/** Never below `headSecs + 1`, never below now — the CLI's rule. */
export function nextPinCreatedAt(headSecs: number, nowSecs: number): number {
  return Math.max(headSecs + 1, nowSecs);
}

/**
 * What the target is, from its own shape — the TypeScript twin of
 * `pin_target_kind_of`. A caller never labels a target, so a mislabelled pin
 * is impossible rather than merely refused.
 */
export function pinTargetKindOf(
  target: string,
): { ok: true; kind: PinTargetKind } | { ok: false; error: string } {
  const asFile = pinTargetError(target, "file");
  if (asFile === null) return { ok: true, kind: "file" };
  if (pinTargetError(target, "folder") === null) {
    return { ok: true, kind: "folder" };
  }
  // The file reading is the more specific refusal of the two — it names the
  // keep case and the grammar — so it is what the caller is told.
  return { ok: false, error: asFile };
}

/**
 * The rank that puts `target` at `index` among `pinned`, skipping the target's
 * own row.
 *
 * Skipping it is what makes "move to the top" move anything: counting its own
 * row as a neighbour would mint a rank between itself and the row above and
 * leave it exactly where it was.
 */
export function rankAt(
  pinned: readonly PinRow[],
  target: string,
  index?: number,
): string {
  const others = pinned.filter((row) => row.target !== target);
  const at = Math.min(index ?? others.length, others.length);
  const after = at > 0 ? others[at - 1] : undefined;
  const before = others[at];
  return rankBetween(after?.rank ?? null, before?.rank ?? null);
}

/** Sign and publish one pin op, then append it to the cached read. */
export async function publishPinOp(
  queryClient: QueryClient,
  coordinate: string,
  op: PinOp,
): Promise<RelayEvent> {
  const key = artifactPinsQueryKey(coordinate);
  const read = queryClient.getQueryData<ArtifactPinsRead>(key);
  const head = read?.latestByTarget[op.content.target] ?? 0;
  const now = Math.floor(Date.now() / 1_000);
  const createdAt = nextPinCreatedAt(head, now);
  if (createdAt > now + MAX_FUTURE_SKEW_SECS) {
    throw new Error(
      `The latest change to this pin is stamped ${head - now}s in the future; try again in a moment.`,
    );
  }
  const event = await signRelayEvent({
    kind: KIND_PROJECT_ARTIFACT_PIN_OP,
    content: encodePinOpContent(op),
    createdAt,
    tags: pinOpTags(coordinate, op),
  });
  const accepted = await relayClient.publishEvent(
    event,
    "Timed out saving the pin.",
    "The relay refused the pin.",
  );
  queryClient.setQueryData<ArtifactPinsRead>(key, (previous) =>
    pinsReadFromEvents(
      coordinate,
      op.repo,
      [...(previous?.events ?? []), accepted],
      previous?.truncated ?? false,
    ),
  );
  void queryClient.invalidateQueries({ queryKey: key });
  return accepted;
}

export type ArtifactPinMutations = {
  /** Pin `target`, appending it to the order unless `index` says otherwise. */
  pin: (target: string, index?: number) => Promise<RelayEvent>;
  /** Unpin `target`, keeping its rank so re-pinning puts it back. */
  unpin: (target: string) => Promise<RelayEvent>;
  /** Move a pinned `target` to `index` among the pinned rows. */
  move: (target: string, index: number) => Promise<RelayEvent>;
};

/** Reference-stable pin callbacks for one project and repository. */
export function useArtifactPinMutations(
  coordinate: string | null,
  repo: string | null,
): ArtifactPinMutations {
  const queryClient = useQueryClient();
  return React.useMemo(() => {
    const require = (): { coordinate: string; repo: string } => {
      if (coordinate === null || repo === null) {
        throw new Error(
          "This project has no agents repository to pin anything in yet.",
        );
      }
      return { coordinate, repo };
    };
    const pinnedRows = (): PinRow[] => {
      const read = queryClient.getQueryData<ArtifactPinsRead>(
        artifactPinsQueryKey(coordinate ?? "none"),
      );
      return (read?.digest.pins ?? []).filter((row) => row.pinned);
    };
    const rowOf = (target: string): PinRow | undefined => {
      const read = queryClient.getQueryData<ArtifactPinsRead>(
        artifactPinsQueryKey(coordinate ?? "none"),
      );
      return read?.digest.pins.find((row) => row.target === target);
    };
    return {
      async pin(target, index) {
        const { coordinate, repo } = require();
        const classified = pinTargetKindOf(target);
        if (!classified.ok) throw new Error(classified.error);
        // Re-pinning something that was pinned before keeps where it was,
        // unless the caller asked for a position.
        const previous = rowOf(target);
        const rank =
          index === undefined && previous
            ? previous.rank
            : rankAt(pinnedRows(), target, index);
        return publishPinOp(queryClient, coordinate, {
          repo,
          content: {
            op: "pin.set",
            target,
            targetKind: classified.kind,
            pinned: true,
            rank,
          },
        });
      },
      async unpin(target) {
        const { coordinate, repo } = require();
        const row = rowOf(target);
        if (!row) throw new Error(`${target} is not pinned in this project.`);
        return publishPinOp(queryClient, coordinate, {
          repo,
          content: {
            op: "pin.set",
            target,
            targetKind: row.targetKind,
            pinned: false,
            // Kept, so re-pinning puts it back rather than at the end.
            rank: row.rank,
          },
        });
      },
      async move(target, index) {
        const { coordinate, repo } = require();
        if (!rowOf(target)) {
          throw new Error(`${target} is not pinned in this project.`);
        }
        return publishPinOp(queryClient, coordinate, {
          repo,
          content: {
            op: "pin.rank",
            target,
            rank: rankAt(pinnedRows(), target, index),
          },
        });
      },
    };
  }, [coordinate, queryClient, repo]);
}
