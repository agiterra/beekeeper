import { createFileRoute, redirect } from "@tanstack/react-router";

/**
 * `/agent-progress` used to be its own screen; Agent progress is now a tab on
 * the Dashboard. The path stays so deep links keep resolving.
 */
export const Route = createFileRoute("/agent-progress")({
  beforeLoad: () => {
    throw redirect({
      to: "/",
      search: { tab: "agent-progress" },
      replace: true,
    });
  },
});
