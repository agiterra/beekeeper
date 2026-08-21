export const SECTION_ICON_BUTTON_CLASS =
  "sidebar-section-action flex size-6 items-center justify-center rounded-[4px] p-1 text-sidebar-foreground/50 transition-colors hover:bg-sidebar-border/35 hover:text-sidebar-foreground focus-visible:bg-sidebar-border/35 focus-visible:text-sidebar-foreground focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-sidebar-ring [&>svg]:size-4 [&>svg]:shrink-0";

export const SECTION_ACTION_VISIBILITY_CLASS =
  "opacity-0 transition-opacity group-hover/sidebar-section:opacity-100 group-focus-within/sidebar-section:opacity-100 group-data-[section-actions-open=true]/sidebar-section:opacity-100";

// The shared collapsible section-label style: title first, with a small
// chevron that fades in on section hover/focus. Used by every sidebar
// section header (Channels, Forums, DMs, custom sections) and the project
// subsection headers, so collapse affordances look identical everywhere.
/** Sidebar scroll-anchor wrapper; with a community rail the sidebar bleeds
 * left under the rail so the glass surfaces meet without a seam. */
export function sidebarScrollAnchorClass(hasCommunityRail: boolean): string {
  return `relative flex min-h-0 flex-1 flex-col overflow-hidden ${
    hasCommunityRail ? "md:-ml-[11px] md:w-[calc(100%+11px)]" : ""
  }`;
}

export const SECTION_LABEL_BUTTON_CLASS =
  "group/section-label flex w-fit max-w-[calc(100%-3rem)] cursor-pointer appearance-none items-center gap-1 text-left transition-colors hover:text-sidebar-foreground focus-visible:text-sidebar-foreground";
export const SECTION_LABEL_CHEVRON_CLASS =
  "relative size-2.5 shrink-0 text-current opacity-0 transition-[color,opacity] group-hover/sidebar-section:opacity-100 group-hover/section-label:opacity-100 group-focus-within/sidebar-section:opacity-100 group-focus-visible/section-label:opacity-100 group-data-[section-actions-open=true]/sidebar-section:opacity-100";
export const SECTION_LABEL_CHEVRON_ICON_CLASS =
  "absolute left-1/2 top-1/2 size-2.5 -translate-x-1/2 -translate-y-1/2";
