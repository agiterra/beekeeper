import * as React from "react";

import { useAgentProgressCoordination } from "@/features/agent-progress/lib/agentProgressCoordination";
import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import type { CodingSessionProviderReachability } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import type { CodingSessionCommandTarget } from "@/features/coding-sessions/lib/codingSessionCommand";
import type { AgentProgressCoordinationRead } from "@/features/agent-progress/lib/agentProgressCoordination";

/**
 * Resolve one generation's reachability out of a coordination read.
 *
 * Pure and exported so the rule is testable without React: a generation the
 * read never proved is `known: false`, never "unreachable". A partial read, a
 * pre-lease provider, and a session in another channel all land there, and
 * none of them may be rendered as "nobody is answering" — that claim needs
 * positive evidence (the fold saw this exact generation and its lease is not
 * live), not the absence of evidence.
 */
export function codingSessionReachabilityFromRead(
  read: AgentProgressCoordinationRead | null,
  targetKey: string | null,
): CodingSessionProviderReachability {
  if (read === null || targetKey === null) return { known: false };
  for (const session of read.sessions) {
    for (const generation of session.generations) {
      if (generation.targetKey !== targetKey) continue;
      return {
        known: true,
        reachable: generation.reachability === "provider_reachable",
      };
    }
  }
  return { known: false };
}

/** Answers reachability for any generation in one channel. */
export type CodingSessionReachabilityResolver = (
  commandTarget: CodingSessionCommandTarget | null,
) => CodingSessionProviderReachability;

/** What every surface gets before a coordination read exists: no claim. */
export const UNKNOWN_CODING_SESSION_REACHABILITY: CodingSessionReachabilityResolver =
  () => ({ known: false });

/**
 * One channel's reachability resolver.
 *
 * Reuses the coordination fold Pulse and Agent Progress already share (§2 item
 * 36) rather than teaching this surface a second liveness clock: the hook it
 * calls owns lease selection, conservative expiry, and the re-render when the
 * earliest live lease lapses. It is called once per surface and threaded down
 * as a prop, so a workspace with eight executions still makes one read — and
 * so a composer stays renderable without a query client.
 */
export function useCodingSessionReachabilityResolver(
  channelId: string | null,
): CodingSessionReachabilityResolver {
  const channelIds = React.useMemo(
    () => (channelId === null ? [] : [channelId]),
    [channelId],
  );
  const { read } = useAgentProgressCoordination(channelIds, false);
  return React.useMemo(
    () => (commandTarget) =>
      codingSessionReachabilityFromRead(
        read,
        commandTarget === null
          ? null
          : buildCodingSessionTargetKey(commandTarget),
      ),
    [read],
  );
}
