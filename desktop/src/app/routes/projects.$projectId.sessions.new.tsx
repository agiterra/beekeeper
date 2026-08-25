import * as React from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";

import { openNewProjectCodingSessionDialog } from "@/features/coding-sessions/newCodingSessionDialogStore";
import { usePreviewFeatureWarning } from "@/shared/features";

export const Route = createFileRoute("/projects/$projectId/sessions/new")({
  component: ProjectNewCodingSessionRouteComponent,
});

/**
 * The project create flow is a dialog now. This URL survives as an opener so
 * links into it keep working; the window falls back to the project itself,
 * which is what the dialog sits over.
 */
function ProjectNewCodingSessionRouteComponent() {
  usePreviewFeatureWarning("projects");
  const { projectId } = Route.useParams();
  const navigate = useNavigate();
  React.useEffect(() => {
    openNewProjectCodingSessionDialog(projectId);
    void navigate({
      to: "/projects/$projectId",
      params: { projectId },
      replace: true,
    });
  }, [navigate, projectId]);
  return null;
}
