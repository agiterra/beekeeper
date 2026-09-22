import * as React from "react";
import { useQuery } from "@tanstack/react-query";

import { useCommunitiesIfPresent } from "@/features/communities/useCommunities";
import { invokeTauri } from "@/shared/api/tauri";

/** Re-asked rarely: the queue only changes at boot and when the drain runs. */
const STALE_MS = 60_000;

/**
 * Agents this computer renamed whose kind:0 profile the relay has not been
 * told about yet.
 *
 * The per-project naming migration (ledger 246) renames agents at boot, where
 * there is no relay to publish to, so each rename is queued and drained on the
 * next workspace apply. Until then the app says `Lead` and everyone else still
 * sees `Lead 4` — a real split, and one nothing on screen would otherwise
 * mention. The directory row says it.
 *
 * Answers an empty set while the community is unknown — including when this
 * list is mounted outside the community provider: silence is not a claim that
 * the relay is up to date, it is the absence of one, and the row renders
 * nothing rather than a guess.
 */
export function usePendingProfileRepublish(): ReadonlySet<string> {
  const relayUrl = useCommunitiesIfPresent()?.activeCommunity?.relayUrl;
  const query = useQuery({
    queryKey: ["pending-profile-republish", relayUrl ?? ""],
    queryFn: () =>
      invokeTauri<string[]>("pending_profile_republish_pubkeys", {
        workspaceRelay: relayUrl ?? "",
      }),
    enabled: Boolean(relayUrl),
    staleTime: STALE_MS,
  });
  return React.useMemo(
    () => new Set((query.data ?? []).map((pubkey) => pubkey.toLowerCase())),
    [query.data],
  );
}
