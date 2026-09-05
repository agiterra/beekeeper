import { Link } from "@tanstack/react-router";

import {
  type DashboardTab,
  dashboardTabSearch,
} from "@/features/dashboard/lib/dashboardTabs";
import { PROJECT_TAB_TRIGGER_CLASS } from "@/features/projects/ui/ProjectWorkspaceTabList";
import { cn } from "@/shared/lib/cn";

type DashboardTabDescriptor = {
  id: DashboardTab;
  label: string;
  /**
   * The test id each tab trigger carries. These are the ids the four sidebar
   * rows used to carry, so every spec that clicked its way to Pulse, Agent
   * progress or Agents keeps working against the tab strip.
   */
  testId: string;
};

const ALL_TABS: readonly DashboardTabDescriptor[] = [
  { id: "overview", label: "Overview", testId: "open-overview-view" },
  { id: "inbox", label: "Inbox", testId: "open-inbox-view" },
  { id: "pulse", label: "Pulse", testId: "open-pulse-view" },
  {
    id: "agent-progress",
    label: "Agent progress",
    testId: "open-agent-progress-view",
  },
  { id: "roles", label: "Roles", testId: "open-roles-view" },
  { id: "agents", label: "Agents", testId: "open-agents-view" },
];

/**
 * Route-driven tabs under the Dashboard title. Each tab is a real link to
 * `/?tab=…` (overview: no param), so deep links and back/forward work without
 * local state. Switching replaces the whole search on purpose — a profile
 * panel or inbox item open on one tab has no meaning on another.
 */
export function DashboardTabs({
  active,
  showAgentProgress,
  showPulse,
}: {
  active: DashboardTab;
  showAgentProgress: boolean;
  showPulse: boolean;
}) {
  const tabs = ALL_TABS.filter((tab) => {
    if (tab.id === "pulse") return showPulse;
    if (tab.id === "agent-progress") return showAgentProgress;
    return true;
  });
  return (
    <nav
      aria-label="Dashboard sections"
      className="flex h-9 shrink-0 items-stretch gap-1 border-b border-border px-4 sm:px-6"
      data-testid="dashboard-tabs"
    >
      {tabs.map((tab) => {
        const isActive = tab.id === active;
        return (
          <Link
            aria-current={isActive ? "page" : undefined}
            className={cn(
              PROJECT_TAB_TRIGGER_CLASS,
              "inline-flex items-center justify-center",
            )}
            data-active={isActive ? "true" : "false"}
            data-state={isActive ? "active" : "inactive"}
            data-testid={tab.testId}
            key={tab.id}
            search={dashboardTabSearch(tab.id)}
            to="/"
          >
            {tab.label}
          </Link>
        );
      })}
    </nav>
  );
}
