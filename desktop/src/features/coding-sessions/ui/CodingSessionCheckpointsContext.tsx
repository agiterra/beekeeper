/**
 * The session's verified turn checkpoints, for every turn and the Diff
 * surface beneath one workspace (SV-28/SV-30).
 *
 * A context keyed by `(generationId, turnId)` rather than a field on the turn
 * model: the transcript's turn objects are shared and kept identical across
 * views (SV-100), and a checkpoint arriving must not rebuild them. A turn
 * reads its own entry here; the fold hands back the same entry object for an
 * unchanged checkpoint, so a turn re-renders only when its own changes.
 *
 * A React context, not a module-level cache: nothing outlives the tree, so
 * `resetCommunityState()` has nothing to reset.
 */
import * as React from "react";

import {
  type CodingSessionCheckpointsRead,
  useCodingSessionCheckpoints,
} from "../hooks/useCodingSessionCheckpoints";
import type {
  CodingSessionCheckpointEntry,
  CodingSessionGenerationCheckpoints,
} from "../lib/codingSessionCheckpoints";
import type { CodingSessionUmbrellaRecord } from "../lib/codingSessionTypes";
import {
  type CodingSessionSurfaceCtx,
  CodingSessionSurfaceCtxProvider,
} from "./surfaces/codingSessionSurfaceContext";

const CodingSessionCheckpointsContext =
  React.createContext<CodingSessionCheckpointsRead | null>(null);

/** Reads the umbrella's checkpoints once and provides them beneath. */
export function CodingSessionCheckpointsProvider({
  channelId,
  children,
  umbrella,
}: {
  channelId: string;
  children: React.ReactNode;
  umbrella: CodingSessionUmbrellaRecord;
}) {
  const read = useCodingSessionCheckpoints(channelId, umbrella);
  return (
    <CodingSessionCheckpointsContext.Provider value={read}>
      {children}
    </CodingSessionCheckpointsContext.Provider>
  );
}

/** Supplies an already-built read (tests, pop-outs that hold one). */
export const CodingSessionCheckpointsValueProvider =
  CodingSessionCheckpointsContext.Provider;

/** The whole read, or null outside a workspace that provides one. */
export function useCodingSessionCheckpointsRead(): CodingSessionCheckpointsRead | null {
  return React.useContext(CodingSessionCheckpointsContext);
}

/** One generation's checkpoints, from a read. */
export function codingSessionGenerationCheckpointsOf(
  read: CodingSessionCheckpointsRead | null,
  generationId: string | null,
): CodingSessionGenerationCheckpoints | null {
  if (!read || !generationId) return null;
  const scopeKey = read.scopeByGeneration.get(generationId);
  return scopeKey ? (read.fold.byScope.get(scopeKey) ?? null) : null;
}

/** One generation's checkpoints, or null. */
export function useCodingSessionGenerationCheckpoints(
  generationId: string | null,
): CodingSessionGenerationCheckpoints | null {
  return codingSessionGenerationCheckpointsOf(
    useCodingSessionCheckpointsRead(),
    generationId,
  );
}

/** One turn's checkpoint, or null when it has none (yet). */
export function useCodingSessionTurnCheckpoint(
  generationId: string | null,
  turnId: string,
): CodingSessionCheckpointEntry | null {
  return (
    useCodingSessionGenerationCheckpoints(generationId)?.byTurnId.get(turnId) ??
    null
  );
}

/**
 * The surface context and the checkpoints read in one element, so a
 * workspace swaps its `CodingSessionSurfaceCtxProvider` for this without
 * re-nesting its body: everything that reads the surface context (the
 * transcript, the Diff surface) also reads the checkpoints.
 */
export function CodingSessionCheckpointedSurfaceCtx({
  channelId,
  children,
  ctx,
  umbrella,
}: {
  channelId: string;
  children: React.ReactNode;
  ctx: CodingSessionSurfaceCtx;
  umbrella: CodingSessionUmbrellaRecord;
}) {
  return (
    <CodingSessionCheckpointsProvider channelId={channelId} umbrella={umbrella}>
      <CodingSessionSurfaceCtxProvider value={ctx}>
        {children}
      </CodingSessionSurfaceCtxProvider>
    </CodingSessionCheckpointsProvider>
  );
}
