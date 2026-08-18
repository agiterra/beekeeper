/**
 * Resolve display profiles for the operators who drove a session's turns.
 *
 * Lives at the workspace level rather than inside the transcript renderer:
 * the umbrella surface mounts one transcript per turn block, so resolving
 * per-transcript would fan a community-scoped profile query out across the
 * whole timeline. Hoisting it makes the lookup once per surface.
 */

import * as React from "react";

import { collectForeignOperatorPubkeys } from "@/features/coding-sessions/lib/codingSessionPromptAttribution";
import { useUsersBatchQuery } from "@/features/profile/hooks";
import type { UserProfileLookup } from "@/features/profile/lib/identity";

/**
 * Profiles for every operator in `items` who is not the viewer, or `undefined`
 * while nothing has resolved.
 *
 * The viewer is excluded because their own prompts render as `"You"` and need
 * no profile — a session driven only by the person reading it therefore issues
 * no query at all.
 */
export function useCodingSessionOperatorProfiles(
  items: readonly unknown[],
  currentUserPubkey: string | null | undefined,
): UserProfileLookup | undefined {
  const operatorPubkeys = React.useMemo(
    () => collectForeignOperatorPubkeys(items, currentUserPubkey),
    [items, currentUserPubkey],
  );
  return useUsersBatchQuery(operatorPubkeys).data?.profiles;
}
