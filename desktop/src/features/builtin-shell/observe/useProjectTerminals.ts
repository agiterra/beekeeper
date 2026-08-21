import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { relayClient } from "@/shared/api/relayClient";
import { useIdentityQuery } from "@/shared/api/hooks";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_SHELL_SESSION } from "@/shared/constants/kinds";

/** One roster member parsed from an announce's arity-4 `p` tag. */
export type RemoteTerminalRosterEntry = {
  /** Lowercase 64-hex pubkey. */
  pubkey: string;
  role: string;
};

/** A member's shared terminal, from its kind:30623 announce. */
export type RemoteTerminal = {
  sessionId: string;
  ownerPubkey: string;
  title: string;
  projectRef: string;
  dims: string | null;
  /** Invited members (`["p", <hex>, "", <role>]` tags). Advisory for UI —
   * the relay and the owner host enforce the actual grants. */
  roster: RemoteTerminalRosterEntry[];
  /** Announce freshness (unix seconds) — old `open` heads read as stale. */
  announcedAt: number;
};

function tagValue(event: RelayEvent, name: string): string | null {
  const values = event.tags
    .filter((tag) => tag[0] === name && typeof tag[1] === "string")
    .map((tag) => tag[1] as string);
  return values.length === 1 ? values[0] : null;
}

/** Parse the invite roster from an announce's arity-4 `p` tags. Malformed
 * entries (wrong arity, non-hex pubkey, unknown role) are skipped — display
 * code must never invent a grant from a tag the relay would have rejected. */
export function rosterFromAnnounce(
  event: RelayEvent,
): RemoteTerminalRosterEntry[] {
  const roster: RemoteTerminalRosterEntry[] = [];
  for (const tag of event.tags) {
    if (tag[0] !== "p" || tag.length < 4) continue;
    const pubkey = typeof tag[1] === "string" ? tag[1].toLowerCase() : "";
    const role = tag[3];
    if (!/^[0-9a-f]{64}$/.test(pubkey)) continue;
    if (role !== "collaborator" && role !== "viewer") continue;
    roster.push({ pubkey, role });
  }
  return roster;
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
      roster: rosterFromAnnounce(event),
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
  const queryClient = useQueryClient();
  const myPubkey = identity.data?.pubkey ?? null;

  // Live announce fan-out: a member opening/closing/renaming a session
  // refreshes the list immediately; the 30s poll below is the fallback for
  // missed events (reconnects, replaceable-head races).
  React.useEffect(() => {
    if (projectAddress === null) return;
    let disposed = false;
    let unsubscribe: (() => void) | null = null;
    void relayClient
      .subscribeLive(
        {
          kinds: [KIND_SHELL_SESSION],
          "#a": [projectAddress],
          since: Math.floor(Date.now() / 1_000),
          limit: 100,
        },
        () => {
          void queryClient.invalidateQueries({
            queryKey: projectTerminalsQueryKey(projectAddress),
          });
        },
      )
      .then((handle) => {
        if (disposed) handle?.();
        else unsubscribe = handle ?? null;
      })
      .catch(() => {
        // Poll fallback still runs.
      });
    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, [projectAddress, queryClient]);

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

export const remoteTerminalsIndexQueryKey = [
  "projects",
  "shared-terminals",
  "index",
];

/**
 * All members' open shared terminals, bucketed by project address — one
 * query + one live subscription for every sidebar project group (per-group
 * hooks would be hooks-in-a-loop). The relay withholds announces of private
 * projects the viewer isn't admitted to, so bucketing is display-only.
 */
export function useRemoteTerminalsIndex(
  enabled: boolean,
): ReadonlyMap<string, RemoteTerminal[]> {
  const identity = useIdentityQuery();
  const queryClient = useQueryClient();
  const myPubkey = identity.data?.pubkey ?? null;

  React.useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    let unsubscribe: (() => void) | null = null;
    void relayClient
      .subscribeLive(
        {
          kinds: [KIND_SHELL_SESSION],
          since: Math.floor(Date.now() / 1_000),
          limit: 100,
        },
        () => {
          void queryClient.invalidateQueries({
            queryKey: remoteTerminalsIndexQueryKey,
          });
        },
      )
      .then((handle) => {
        if (disposed) handle?.();
        else unsubscribe = handle ?? null;
      })
      .catch(() => {
        // Poll fallback still runs.
      });
    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, [enabled, queryClient]);

  const query = useQuery({
    queryKey: remoteTerminalsIndexQueryKey,
    enabled,
    refetchInterval: 30_000,
    queryFn: async () => {
      const events = await relayClient.fetchEvents({
        kinds: [KIND_SHELL_SESSION],
        limit: 200,
      });
      const addresses = new Set<string>();
      for (const event of events) {
        const address = tagValue(event, "a");
        if (address) addresses.add(address);
      }
      const index = new Map<string, RemoteTerminal[]>();
      for (const address of addresses) {
        const terminals = remoteTerminalsFromEvents(events, address, myPubkey);
        if (terminals.length > 0) index.set(address, terminals);
      }
      return index;
    },
  });

  return query.data ?? EMPTY_INDEX;
}

const EMPTY_INDEX: ReadonlyMap<string, RemoteTerminal[]> = new Map();
