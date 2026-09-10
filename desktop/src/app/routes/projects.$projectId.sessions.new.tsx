import * as React from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";

import { requestProjectCodingSessionFounding } from "@/features/coding-sessions/newCodingSessionDialogStore";
import { usePreviewFeatureWarning } from "@/shared/features";

export const Route = createFileRoute("/projects/$projectId/sessions/new")({
  component: ProjectNewCodingSessionRouteComponent,
});

/**
 * A project session is founded on the click now. This URL survives as an
 * opener so links into it keep working: it writes the project founding
 * request (the host in the app shell resolves the channel, founds the session
 * and opens its page) and falls back to the project itself meanwhile.
 */
function ProjectNewCodingSessionRouteComponent() {
  usePreviewFeatureWarning("projects");
  const { projectId } = Route.useParams();
  const navigate = useNavigate();
  React.useEffect(() => {
    requestProjectCodingSessionFounding(projectId);
    void navigate({
      to: "/projects/$projectId",
      params: { projectId },
      replace: true,
    });
  }, [navigate, projectId]);
  return null;
}
