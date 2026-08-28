import type * as React from "react";

import type { DashboardTab } from "@/features/dashboard/lib/dashboardTabs";
import { DashboardOverview } from "@/features/dashboard/ui/DashboardOverview";
import { DashboardTabs } from "@/features/dashboard/ui/DashboardTabs";

/**
 * The Dashboard page: a title, the tab strip, and one body.
 *
 * The bodies are the four screens that used to be routes of their own —
 * passed in already wired, because the data each needs (channel ids for the
 * inbox, the coordination read for agent progress) is the route's business,
 * not this layout's. Only the selected body mounts; a tab body that keeps
 * running hidden would keep polling for a surface nobody is looking at.
 */
export function DashboardScreen({
  active,
  agentProgress,
  agents,
  inbox,
  pulse,
  showAgentProgress,
  showPulse,
}: {
  active: DashboardTab;
  agentProgress: React.ReactNode;
  agents: React.ReactNode;
  inbox: React.ReactNode;
  pulse: React.ReactNode;
  showAgentProgress: boolean;
  showPulse: boolean;
}) {
  return (
    <div
      className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden"
      data-testid="dashboard-screen"
    >
      <header
        className="flex h-11 shrink-0 items-center px-4 sm:px-6"
        data-tauri-drag-region
      >
        <h1 className="text-base font-semibold tracking-tight">Dashboard</h1>
      </header>
      <DashboardTabs
        active={active}
        showAgentProgress={showAgentProgress}
        showPulse={showPulse}
      />
      <div
        className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden"
        data-testid={`dashboard-body-${active}`}
      >
        {active === "inbox" ? (
          inbox
        ) : active === "pulse" ? (
          pulse
        ) : active === "agent-progress" ? (
          agentProgress
        ) : active === "agents" ? (
          agents
        ) : (
          <DashboardOverview
            showAgentProgress={showAgentProgress}
            showPulse={showPulse}
          />
        )}
      </div>
    </div>
  );
}
