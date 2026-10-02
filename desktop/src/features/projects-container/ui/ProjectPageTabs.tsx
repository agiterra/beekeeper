import { Link } from "@tanstack/react-router";

import { agentsRepoCopy } from "@/features/agents-repo/lib/agentsRepoCopy";
import { LOCAL_GENERAL_ID } from "@/features/projects-container/lib/projectContainerModel";
import { PROJECT_TAB_TRIGGER_CLASS } from "@/features/projects/ui/ProjectWorkspaceTabList";
import { cn } from "@/shared/lib/cn";

/** The project page's tab vocabulary. Overview/Pulse are the URL `tab` search
 * param on `/projects/$projectId`; the To-Do, Agents, Actions and Roles
 * (`packs`) tabs are their own paths. */
export type ProjectPageTab =
  | "overview"
  | "pulse"
  | "todos"
  | "agents"
  | "actions"
  | "packs"
  | "files";

export function parseProjectPageTab(value: unknown): ProjectPageTab {
  return value === "pulse" ? "pulse" : "overview";
}

type SearchTab = { id: "overview" | "pulse"; label: string };
type PathTab = {
  id: "todos" | "agents" | "actions" | "packs" | "files";
  label: string;
  to:
    | "/projects/$projectId/todos"
    | "/projects/$projectId/agents"
    | "/projects/$projectId/actions"
    | "/projects/$projectId/packs"
    | "/projects/$projectId/files";
};

const PATH_TABS: readonly PathTab[] = [
  // To-Do next to Overview: the shared list of what is left is the thing a
  // member checks most often after the project's state.
  { id: "todos", label: "To-Do", to: "/projects/$projectId/todos" },
  // Agents: who is working here and why. It replaced Contributors, whose
  // path now redirects here.
  { id: "agents", label: "Agents", to: "/projects/$projectId/agents" },
  // Actions: the agents repository's `actions.yml` entries as the relay holds
  // them, their runs, and the approvals and host steps each run proves.
  { id: "actions", label: "Actions", to: "/projects/$projectId/actions" },
  // The tab reads "Roles" — the page answers "what is this role for, which
  // version of its instructions is here". Its id, path and testid stay
  // `packs` so routes, links and existing selectors keep working.
  { id: "packs", label: "Roles", to: "/projects/$projectId/packs" },
  // Artifacts: the agents repository itself — plans, documents, roles,
  // skills, team and actions — edited as shared drafts and committed to main
  // (spec § 4.12). The tab read "Files" until the documents tree landed;
  // "Artifacts" is the name for all of it, and `plans` and `docs` are the two
  // trees a project writes in. The id, path and testid stay `files` so
  // routes, remembered routes and existing selectors keep working.
  {
    id: "files",
    label: agentsRepoCopy.tabLabel,
    to: "/projects/$projectId/files",
  },
];

/**
 * Route-driven tabs under the project header: Overview and Pulse are search-
 * param links on `/projects/$projectId` (unchanged); Agents and Roles
 * are real paths, because each names the project in its own URL rather than
 * asking a Dashboard-style picker which one.
 *
 * The strip renders for any real project — not only when Pulse is on, since
 * Agents and Roles have nothing to do with that feature gate. The one
 * project with no coordinate to link into (the local General placeholder,
 * before a real `general` head is published) gets no tab strip at all.
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
  if (projectId === LOCAL_GENERAL_ID) return null;
  const searchTabs: readonly SearchTab[] = showPulse
    ? [
        { id: "overview", label: "Overview" },
        { id: "pulse", label: "Pulse" },
      ]
    : [{ id: "overview", label: "Overview" }];
  return (
    <nav
      aria-label="Project sections"
      // The tabs never shrink, so at 250% zoom the strip is wider than the
      // page and used to widen the whole project screen with it — every card
      // under it then scrolled sideways to reach a tab bar nobody was
      // reading. Scrolling it in its own box keeps the overflow where it
      // belongs (`role-packs-project.spec.ts`, the 250% zoom matrix).
      className="mb-6 flex h-9 min-w-0 items-stretch gap-1 overflow-x-auto border-b border-border"
      data-testid="project-page-tabs"
    >
      {searchTabs.map((tab) => {
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
      {PATH_TABS.map((tab) => {
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
            to={tab.to}
          >
            {tab.label}
          </Link>
        );
      })}
    </nav>
  );
}
