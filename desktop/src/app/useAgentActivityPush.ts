import * as React from "react";
import { isTauri, invoke } from "@tauri-apps/api/core";

import {
  getActiveTurnsForAgent,
  useActiveAgentTurnsByChannel,
} from "@/features/agents/activeAgentTurnsStore";
import {
  useManagedAgentsQuery,
  useRelayAgentsQuery,
} from "@/features/agents/hooks";
import { normalizePubkey, truncatePubkey } from "@/shared/lib/pubkey";
import type { Channel } from "@/shared/api/types";

/**
 * One managed-agent turn, as the agent host is told about it.
 *
 * Mirrors the Rust `PushedActivity` struct in
 * `crates/beekeeper-host/src/activity.rs`.
 */
export type PushedAgentActivity = {
  activityId: string;
  agentName: string;
  /**
   * The agent's identity. The menu spans every project and channel on this
   * computer, and names are unique per project rather than per computer
   * (ledger 246), so two rows can both say `Builder`; the native menu appends
   * a truncated key to whichever names actually collide.
   */
  agentPubkey: string;
  channelId: string;
  channelName: string;
  /**
   * When the turn started, milliseconds since the epoch.
   *
   * An absolute instant, **not** a formatted duration. This used to be an
   * `elapsed` string re-derived here against a 1 Hz ticker, which meant the
   * whole webview re-rendered every second to keep a menu label moving. The
   * menu bar app now formats from its own clock, so this is sent once per turn
   * rather than once per second.
   */
  startedAtMs: number;
  /** Finished work rather than a running turn. */
  recent?: boolean;
};

/** How many completed turns to keep offering as "Recent". */
const MAX_RECENT_ACTIVITIES = 5;

/**
 * How often to re-push, well inside the host's lease.
 *
 * The rows expire if this stops, which is how they disappear when Beekeeper
 * quits — those agents died with it, and a menu still showing them would be
 * claiming a dead agent is working. So this is a heartbeat, not an
 * on-change-only send: a push that fails once must not silently drop the rows
 * until something else happens to change.
 */
const PUSH_INTERVAL_MS = 5_000;

/**
 * Tells the agent host which managed agents are working, so the menu bar app
 * can show a complete picture of this machine.
 *
 * The host knows its own coding sessions. It cannot know about managed agents
 * — they are this app's children, and they still die with it — and it cannot
 * know channel or agent *names* at all, which live on the relay. This hook is
 * the only place that has all three, so it is the one that says.
 */
export function useAgentActivityPush({
  channels,
}: {
  channels: Channel[];
}): void {
  const activeTurns = useActiveAgentTurnsByChannel();
  const managedAgents = useManagedAgentsQuery().data;
  const relayAgents = useRelayAgentsQuery().data;
  const previousRef = React.useRef(new Map<string, PushedAgentActivity>());
  const [recent, setRecent] = React.useState<PushedAgentActivity[]>([]);

  const activities = React.useMemo<PushedAgentActivity[]>(() => {
    const channelNames = new Map(
      channels.map((channel) => [channel.id, channel.name]),
    );
    const agentNames = new Map<string, string>();
    for (const agent of [...(managedAgents ?? []), ...(relayAgents ?? [])]) {
      agentNames.set(normalizePubkey(agent.pubkey), agent.name);
    }

    return activeTurns.flatMap((channelTurn) =>
      channelTurn.agentPubkeys.map((pubkey) => {
        const agentTurn = getActiveTurnsForAgent(pubkey).find(
          (turn) => turn.channelId === channelTurn.channelId,
        );
        return {
          activityId: `${channelTurn.channelId}:${normalizePubkey(pubkey)}`,
          agentPubkey: normalizePubkey(pubkey),
          agentName:
            agentNames.get(normalizePubkey(pubkey)) ??
            `Agent ${truncatePubkey(pubkey)}`,
          channelId: channelTurn.channelId,
          channelName:
            channelNames.get(channelTurn.channelId) ?? "Unknown channel",
          startedAtMs: agentTurn?.anchorAt ?? channelTurn.anchorAt,
        };
      }),
    );
  }, [activeTurns, channels, managedAgents, relayAgents]);

  // Turns that were running and are not any more become "Recent".
  React.useEffect(() => {
    const current = new Map(
      activities.map((activity) => [activity.activityId, activity]),
    );
    const completed = [...previousRef.current.entries()]
      .filter(([activityId]) => !current.has(activityId))
      .map(([, activity]) => ({
        ...activity,
        activityId: `recent:${activity.activityId}:${Date.now()}`,
        recent: true,
      }));
    if (completed.length > 0) {
      setRecent((existing) =>
        [...completed, ...existing].slice(0, MAX_RECENT_ACTIVITIES),
      );
    }
    previousRef.current = current;
  }, [activities]);

  React.useEffect(() => {
    if (!isTauri()) return;
    const rows = [...activities, ...recent];
    let disposed = false;

    const push = () => {
      if (disposed) return;
      void invoke("push_agent_activity", { rows }).catch((error) => {
        // Logged, never thrown: the agent host not being installed is an
        // ordinary state on most machines, and this app must not behave
        // differently because of it.
        console.debug(
          "Could not tell the agent host about active agents",
          error,
        );
      });
    };

    push();
    const timer = setInterval(push, PUSH_INTERVAL_MS);
    return () => {
      disposed = true;
      clearInterval(timer);
    };
  }, [activities, recent]);
}
