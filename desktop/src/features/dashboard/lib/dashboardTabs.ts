/**
 * The Dashboard's tab vocabulary — the URL `tab` search param on `/`.
 *
 * The overview is the default and carries no param, exactly like the project
 * page's overview tab. Every other value names one of the surfaces the
 * Dashboard absorbed: the old `/`, `/pulse`, `/agent-progress`, `/agents`
 * routes.
 */
export type DashboardTab =
  | "overview"
  | "inbox"
  | "pulse"
  | "agent-progress"
  | "roles"
  | "agents";

// "roles" is deliberately absent: no tab strip trigger renders it anymore
// (§F — the content is entirely project-scoped, and `/projects/$projectId/packs`
// is where it lives now). It stays in `DashboardTab` and is still accepted by
// `isDashboardTab` below, so an old `?tab=roles` URL still parses as "roles"
// and reaches the redirect in `app/routes/index.tsx` instead of silently
// falling back to the overview.
export const DASHBOARD_TABS: readonly DashboardTab[] = [
  "overview",
  "inbox",
  "pulse",
  "agent-progress",
  "agents",
];

export function isDashboardTab(value: unknown): value is DashboardTab {
  return (
    typeof value === "string" &&
    ((DASHBOARD_TABS as readonly string[]).includes(value) || value === "roles")
  );
}

/**
 * Resolve the requested tab from a route search object.
 *
 * `?item=` is an inbox deep link (notification clicks, `beekeeper://` links)
 * and predates the Dashboard, so a location that names an item but no tab is
 * read as the inbox — the item would be meaningless on any other tab.
 */
export function parseDashboardTab(
  search: Record<string, unknown> | undefined,
): DashboardTab {
  const tab = search?.tab;
  if (isDashboardTab(tab)) return tab;
  if (typeof search?.item === "string" && search.item.length > 0) {
    return "inbox";
  }
  return "overview";
}

/**
 * The tab that actually renders once preview gates are applied. A requested
 * tab whose feature is off falls back to the overview rather than to an empty
 * body, so the URL never claims a surface the build is hiding.
 */
export function resolveDashboardTab(
  requested: DashboardTab,
  enabled: { pulse: boolean; agentProgress: boolean },
): DashboardTab {
  if (requested === "pulse" && !enabled.pulse) return "overview";
  if (requested === "agent-progress" && !enabled.agentProgress) {
    return "overview";
  }
  return requested;
}

/** The search patch that selects a tab; the overview clears the param. */
export function dashboardTabSearch(tab: DashboardTab): {
  tab?: Exclude<DashboardTab, "overview">;
} {
  return tab === "overview" ? {} : { tab };
}
