import { createFileRoute, redirect } from "@tanstack/react-router";

/**
 * The Contributors tab became the Agents tab; its seat history is the Agents
 * tab's "Previously here" section. The path stays so existing links resolve.
 */
export const Route = createFileRoute("/projects/$projectId/contributors")({
  beforeLoad: ({ params }) => {
    throw redirect({
      to: "/projects/$projectId/agents",
      params: { projectId: params.projectId },
      replace: true,
    });
  },
});
