import { useQuery } from "@tanstack/react-query";

import { relayClient } from "@/shared/api/relayClient";
import { useIdentityQuery } from "@/shared/api/hooks";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_SHELL_SESSION } from "@/shared/constants/kinds";

/** A member's shared terminal, from its kind:30623 announce. */
export type RemoteTerminal = {
  sessionId: string;
  ownerPubkey: string;
  title: string;
  projectRef: string;
  dims: string | null;
  /** Announce freshness (unix seconds) — old `open` heads read as stale. */
  announcedAt: number;
};

function tagValue(event: RelayEvent, name: string): string | null {
  const values = event.tags
    .filter((tag) => tag[0] === name && typeof tag[1] === "string")
    .map((tag) => tag[1] as string);
  return values.length === 1 ? values[0] : null;
}

export function remoteTerminalsFromEvents(
  events: readonly RelayEvent[],
  projectAddress: string,
  excludeOwner: string | null,
): RemoteTerminal[] {
  const terminals: RemoteTerminal[] = [];
  for (const event of events) {
    if (event.kind !== KIND_SHELL_SESSION) continue;
    if (tagValue(event, "status") !== "open") continue;
    if (tagValue(event, "a") !== projectAddress) continue;
    const sessionId = tagValue(event, "d");
    if (!sessionId) continue;
    if (
      excludeOwner &&
      event.pubkey.toLowerCase() === excludeOwner.toLowerCase()
    ) {
      continue;
    }
    terminals.push({
      sessionId,
      ownerPubkey: event.pubkey,
      title: tagValue(event, "title") ?? "terminal",
      projectRef: projectAddress,
      dims: tagValue(event, "dims"),
      announcedAt: event.created_at,
    });
  }
  terminals.sort((a, b) =>
    a.title.localeCompare(b.title, undefined, { sensitivity: "base" }),
  );
  return terminals;
}

export const projectTerminalsQueryKey = (projectAddress: string) => [
  "projects",
  "shared-terminals",
  projectAddress,
];

/**
 * The other members' open shared terminals in a project, from their 30623
 * announces (the relay withholds announces of private projects the viewer
 * isn't admitted to). The viewer's own sessions come from
 * `list_shell_sessions` instead and are excluded here.
 */
export function useProjectTerminals(projectAddress: string | null) {
  const identity = useIdentityQuery();
  const myPubkey = identity.data?.pubkey ?? null;
  return useQuery({
    queryKey: projectTerminalsQueryKey(projectAddress ?? "none"),
    enabled: projectAddress !== null,
    refetchInterval: 30_000,
    queryFn: async () => {
      const events = await relayClient.fetchEvents({
        kinds: [KIND_SHELL_SESSION],
        "#a": [projectAddress ?? ""],
        limit: 100,
      });
      return remoteTerminalsFromEvents(events, projectAddress ?? "", myPubkey);
    },
  });
}
