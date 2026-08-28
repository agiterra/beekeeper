import { createFileRoute, redirect } from "@tanstack/react-router";

import { forwardProfilePanelSearch } from "@/features/profile/ui/UserProfilePanelUtils";

/**
 * `/agents` used to be its own screen; Agents is now a tab on the Dashboard.
 * The path stays so deep links keep resolving — they land on `/?tab=agents`
 * with any profile-panel keys carried across.
 */
export const Route = createFileRoute("/agents")({
  beforeLoad: ({ search }) => {
    throw redirect({
      to: "/",
      search: { tab: "agents", ...forwardProfilePanelSearch(search) },
      replace: true,
    });
  },
});
