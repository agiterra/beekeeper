import { createFileRoute, redirect } from "@tanstack/react-router";

import { forwardProfilePanelSearch } from "@/features/profile/ui/UserProfilePanelUtils";

/**
 * `/pulse` used to be its own screen; Pulse is now a tab on the Dashboard.
 * The path stays so deep links (runbooks, copied URLs, `goProfile` callers
 * from older builds) keep resolving — they land on `/?tab=pulse` with any
 * profile-panel keys carried across.
 */
export const Route = createFileRoute("/pulse")({
  beforeLoad: ({ search }) => {
    throw redirect({
      to: "/",
      search: { tab: "pulse", ...forwardProfilePanelSearch(search) },
      replace: true,
    });
  },
});
