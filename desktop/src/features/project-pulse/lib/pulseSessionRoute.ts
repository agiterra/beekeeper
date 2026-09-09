/**
 * From a Pulse session to the route that opens its execution.
 *
 * The declared-work section names a session; the coding-session screen is
 * addressed by `(channelId, generationId)`. Nothing in the Pulse digest is
 * that pair, so the join is done here, once, from facts that are already
 * proven: the channel the declared-work response returned for the session, and
 * the session's own authority-proven generations.
 *
 * Two rules keep this from inventing a destination:
 *
 * - The generation is the session's **current** one when the fold marked one
 *   current, else the newest by `statusAt` (nulls last, ties by list order).
 *   A guess would open somebody else's transcript.
 * - A session with no generation, or one whose `targetKey` this client cannot
 *   decode, resolves to `null` — the surface then says no execution is
 *   recorded to open rather than offering a control that goes nowhere.
 *
 * Pure and side-effect free: it composes an identifier, it does not navigate,
 * read, or publish.
 */
import { buildCodingSessionTranscriptGenerationId } from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import { decodeCodingSessionTargetKey } from "@/features/roles/lib/rolePackProvenance";

import type { PulseDigestGeneration, PulseDigestSession } from "./pulseFold.ts";

/** The route parameters `goCodingSession` takes, and nothing else. */
export type PulseSessionRouteParams = {
  channelId: string;
  generationId: string;
};

/**
 * The generation this session's "Open session" control should point at.
 *
 * Exported because the choice is the interesting half: a caller that wants to
 * say *which* generation it opened reads the same one this module routes to.
 */
export function pulseSessionRouteGeneration(
  session: PulseDigestSession,
): PulseDigestGeneration | null {
  const generations = session.generations ?? [];
  if (generations.length === 0) return null;
  const current = generations.find((generation) => generation.current === true);
  if (current) return current;
  let newest: PulseDigestGeneration | null = null;
  for (const generation of generations) {
    if (newest === null) {
      newest = generation;
      continue;
    }
    const at = generation.statusAt;
    const best = newest.statusAt;
    if (at === null) continue;
    if (best === null || at > best) newest = generation;
  }
  return newest;
}

/**
 * `(channelId, generationId)` for one Pulse session, or `null` when this
 * client cannot name an execution to open.
 *
 * `channelId` is the caller's — the declared-work response carries the channel
 * each session was read from, and the digest session does not. Passing it in
 * keeps this function from guessing a channel out of the project's floor.
 */
export function pulseSessionRouteParams(input: {
  session: PulseDigestSession;
  channelId: string;
}): PulseSessionRouteParams | null {
  const { channelId, session } = input;
  if (!channelId) return null;
  const generation = pulseSessionRouteGeneration(session);
  if (!generation) return null;
  const target = decodeCodingSessionTargetKey(generation.targetKey);
  if (!target) return null;
  return {
    channelId,
    generationId: buildCodingSessionTranscriptGenerationId(
      channelId,
      generation.providerAuthorityPubkey,
      {
        driver: target.driver,
        instanceId: target.instanceId,
        sessionId: target.sessionId,
        generation: target.generation,
      },
    ),
  };
}
