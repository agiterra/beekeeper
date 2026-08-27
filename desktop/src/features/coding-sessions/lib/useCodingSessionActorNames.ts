/**
 * Resolve the actors seated on an umbrella's executions to display names.
 *
 * The participant model is pure — it has to be, it is folded in tests without
 * a relay — so name resolution is a hook that hands it a resolver. An
 * unresolved actor returns null and the label falls back to the seat's role,
 * which is the honest half: a chip reading `Builder` says less than
 * `Ada · Builder` but never claims a name nobody has.
 */
import * as React from "react";

import { useUsersBatchQuery } from "@/features/profile/hooks";
import type { CodingSessionActorNameResolver } from "./codingSessionUmbrellaModel";
import type { CodingSessionUmbrellaRecord } from "./codingSessionTypes";

/** Every distinct actor seated on this umbrella, lowercase and sorted. */
export function listCodingSessionActorPubkeys(
  umbrella: CodingSessionUmbrellaRecord | null,
): string[] {
  if (!umbrella) return [];
  return [
    ...new Set(
      umbrella.executions
        .map((execution) => execution.activeGeneration.agentRef)
        .filter((actor): actor is string => typeof actor === "string")
        .map((actor) => actor.toLowerCase()),
    ),
  ].sort();
}

/** A stable name resolver for this umbrella's seated actors. */
export function useCodingSessionActorNameResolver(
  umbrella: CodingSessionUmbrellaRecord | null,
): CodingSessionActorNameResolver {
  const actorPubkeys = React.useMemo(
    () => listCodingSessionActorPubkeys(umbrella),
    [umbrella],
  );
  const profiles = useUsersBatchQuery(actorPubkeys).data?.profiles;
  return React.useCallback(
    (actorPubkey: string) => {
      const profile = profiles?.[actorPubkey.toLowerCase()];
      return (
        profile?.displayName?.trim() ||
        profile?.name?.trim() ||
        profile?.nip05Handle?.trim() ||
        null
      );
    },
    [profiles],
  );
}
