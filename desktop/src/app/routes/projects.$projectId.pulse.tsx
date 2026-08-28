import { createFileRoute, redirect } from "@tanstack/react-router";

/**
 * `/projects/$projectId/pulse` used to be its own screen; Pulse is now a tab
 * on the project page. The path stays so deep links (runbooks, copied URLs)
 * keep resolving — they land on `/projects/$projectId?tab=pulse`.
 */
export const Route = createFileRoute("/projects/$projectId/pulse")({
  beforeLoad: ({ params }) => {
    throw redirect({
      to: "/projects/$projectId",
      params: { projectId: params.projectId },
      search: { tab: "pulse" },
      replace: true,
    });
  },
});
