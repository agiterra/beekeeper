import { Link } from "@tanstack/react-router";

import { PROJECT_TAB_TRIGGER_CLASS } from "@/features/projects/ui/ProjectWorkspaceTabList";
import { cn } from "@/shared/lib/cn";

/** The project page's tab vocabulary — the URL `tab` search param. */
export type ProjectPageTab = "overview" | "pulse";

export function parseProjectPageTab(value: unknown): ProjectPageTab {
  return value === "pulse" ? "pulse" : "overview";
}

/**
 * Route-driven tabs under the project header. Each tab is a real link to
 * `/projects/$projectId?tab=…`, so deep links and back/forward work without
 * any local state; the strip only renders when there is more than one tab.
 */
export function ProjectPageTabs({
  projectId,
  active,
  showPulse,
}: {
  projectId: string;
  active: ProjectPageTab;
  showPulse: boolean;
}) {
  if (!showPulse) return null;
  const tabs: Array<{ id: ProjectPageTab; label: string }> = [
    { id: "overview", label: "Overview" },
    { id: "pulse", label: "Pulse" },
  ];
  return (
    <nav
      aria-label="Project sections"
      className="mb-6 flex h-9 items-stretch gap-1 border-b border-border"
      data-testid="project-page-tabs"
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
            data-state={isActive ? "active" : "inactive"}
            data-testid={`project-tab-${tab.id}`}
            key={tab.id}
            params={{ projectId }}
            search={{ tab: tab.id === "overview" ? undefined : tab.id }}
            to="/projects/$projectId"
          >
            {tab.label}
          </Link>
        );
      })}
    </nav>
  );
}
