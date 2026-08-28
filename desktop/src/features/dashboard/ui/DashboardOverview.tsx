import { Link } from "@tanstack/react-router";
import { Activity, Bot, Gauge, Inbox } from "lucide-react";
import * as React from "react";

import { useAgentProgress } from "@/app/agentProgressComposition";
import { useAppShell } from "@/app/AppShellContext";
import {
  agentProgressExecutionsText,
  agentProgressFooterText,
} from "@/features/agent-progress/lib/agentProgressFormat";
import {
  useManagedAgentsQuery,
  usePersonasQuery,
} from "@/features/agents/hooks";
import { isManagedAgentActive } from "@/features/agents/lib/managedAgentControlActions";
import {
  type DashboardTab,
  dashboardTabSearch,
} from "@/features/dashboard/lib/dashboardTabs";
import { resolveUserLabel } from "@/features/profile/lib/identity";
import { useUsersBatchQuery } from "@/features/profile/hooks";
import { useGlobalNotesQuery } from "@/features/pulse/hooks";
import { noteSnippet } from "@/features/pulse/lib/replies";
import { useIdentityQuery } from "@/shared/api/hooks";
import { formatItemTimestamp } from "@/shared/lib/datetime";
import { Card } from "@/shared/ui/card";
import { Skeleton } from "@/shared/ui/skeleton";

const PULSE_PREVIEW_COUNT = 3;

/**
 * One card per surface the Dashboard absorbed, each a link to its tab.
 *
 * Every number here is one the tab itself already shows, read through the
 * same hook — nothing is re-derived, so the card and the tab cannot disagree.
 * When a read is pending or failed the card says so instead of printing 0.
 */
export function DashboardOverview({
  showAgentProgress,
  showPulse,
}: {
  showAgentProgress: boolean;
  showPulse: boolean;
}) {
  return (
    <div className="flex-1 overflow-y-auto overflow-x-hidden overscroll-contain px-4 py-6 sm:px-6">
      <div
        className="mx-auto grid w-full max-w-6xl gap-4 md:grid-cols-2"
        data-testid="dashboard-overview"
      >
        <InboxCard />
        <AgentsCard />
        {showAgentProgress ? <AgentProgressCard /> : null}
        {showPulse ? <PulseCard /> : null}
      </div>
    </div>
  );
}

function OverviewCard({
  children,
  icon,
  tab,
  testId,
  title,
}: {
  children: React.ReactNode;
  icon: React.ReactNode;
  tab: DashboardTab;
  testId: string;
  title: string;
}) {
  return (
    <Card
      asChild
      className="flex flex-col gap-3 p-4 transition-colors hover:bg-muted/40"
      data-testid={testId}
    >
      <Link search={dashboardTabSearch(tab)} to="/">
        <div className="flex items-center gap-2 text-sm font-semibold text-foreground">
          <span className="text-muted-foreground">{icon}</span>
          {title}
        </div>
        <div className="flex min-h-10 flex-col gap-1 text-sm text-muted-foreground">
          {children}
        </div>
      </Link>
    </Card>
  );
}

function InboxCard() {
  const { inboxBadgeCount } = useAppShell();
  return (
    <OverviewCard
      icon={<Inbox className="size-4" />}
      tab="inbox"
      testId="dashboard-card-inbox"
      title="Inbox"
    >
      <p data-testid="dashboard-card-inbox-count">
        {inboxBadgeCount > 0
          ? `${inboxBadgeCount} ${inboxBadgeCount === 1 ? "item needs" : "items need"} you`
          : "Nothing needs you right now"}
      </p>
    </OverviewCard>
  );
}

function AgentsCard() {
  const managedAgentsQuery = useManagedAgentsQuery();
  const personasQuery = usePersonasQuery();
  const managedAgents = managedAgentsQuery.data ?? [];
  const running = managedAgents.filter((agent) =>
    isManagedAgentActive(agent),
  ).length;
  const personaCount = personasQuery.data?.length;
  return (
    <OverviewCard
      icon={<Bot className="size-4" />}
      tab="agents"
      testId="dashboard-card-agents"
      title="Agents"
    >
      {managedAgentsQuery.isPending ? (
        <Skeleton className="h-4 w-32" />
      ) : managedAgentsQuery.isError ? (
        <p>Could not read your agents.</p>
      ) : (
        <p data-testid="dashboard-card-agents-count">
          {managedAgents.length === 0
            ? "No agents set up yet"
            : `${running} running of ${managedAgents.length} ${
                managedAgents.length === 1 ? "agent" : "agents"
              }`}
        </p>
      )}
      {personasQuery.data ? (
        <p className="text-2xs">
          {personaCount === 1 ? "1 persona" : `${personaCount ?? 0} personas`}
        </p>
      ) : null}
    </OverviewCard>
  );
}

/**
 * Its own component so `useAgentProgress` — a coordination read across every
 * readable channel — only runs when the preview is on.
 */
function AgentProgressCard() {
  const state = useAgentProgress();
  const executions = agentProgressExecutionsText(state.aggregate);
  return (
    <OverviewCard
      icon={<Gauge className="size-4" />}
      tab="agent-progress"
      testId="dashboard-card-agent-progress"
      title="Agent progress"
    >
      {state.isLoading ? (
        <Skeleton className="h-4 w-40" />
      ) : (
        <>
          <p data-testid="dashboard-card-agent-progress-counts">
            {agentProgressFooterText(state.aggregate)}
          </p>
          {executions ? <p className="text-2xs">{executions}</p> : null}
          {state.errors.length > 0 ? (
            <p className="text-2xs">
              {state.errors.length === 1
                ? "1 read failed"
                : `${state.errors.length} reads failed`}
            </p>
          ) : null}
        </>
      )}
    </OverviewCard>
  );
}

function PulseCard() {
  const identityQuery = useIdentityQuery();
  const notesQuery = useGlobalNotesQuery(true);
  const notes = React.useMemo(
    () => (notesQuery.data?.notes ?? []).slice(0, PULSE_PREVIEW_COUNT),
    [notesQuery.data],
  );
  const authorPubkeys = React.useMemo(
    () => notes.map((note) => note.pubkey),
    [notes],
  );
  const profilesQuery = useUsersBatchQuery(authorPubkeys);
  const profiles = profilesQuery.data?.profiles;
  return (
    <OverviewCard
      icon={<Activity className="size-4" />}
      tab="pulse"
      testId="dashboard-card-pulse"
      title="Pulse"
    >
      {notesQuery.isPending ? (
        <Skeleton className="h-4 w-48" />
      ) : notesQuery.isError ? (
        <p>Could not load notes.</p>
      ) : notes.length === 0 ? (
        <p>No notes yet</p>
      ) : (
        <ul
          className="flex flex-col gap-1.5"
          data-testid="dashboard-card-pulse-notes"
        >
          {notes.map((note) => (
            <li className="flex min-w-0 items-baseline gap-2" key={note.id}>
              <span className="shrink-0 font-medium text-foreground">
                {resolveUserLabel({
                  pubkey: note.pubkey,
                  currentPubkey: identityQuery.data?.pubkey,
                  profiles,
                })}
              </span>
              <span className="min-w-0 flex-1 truncate">
                {noteSnippet(note.content)}
              </span>
              <span className="shrink-0 text-2xs">
                {formatItemTimestamp(note.createdAt)}
              </span>
            </li>
          ))}
        </ul>
      )}
    </OverviewCard>
  );
}
