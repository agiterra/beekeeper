import * as React from "react";

import {
  closeNewCodingSessionDialog,
  useNewCodingSessionRequest,
} from "../newCodingSessionDialogStore";
import { NewCodingSessionDialog } from "./NewCodingSessionDialog";

/**
 * The project flow pulls in the whole projects-container feature — its
 * containers query, its channel partitioning, its repo-checkout resolution.
 * None of that should load for someone who never opens a project session, so
 * it arrives only when a project request does.
 */
const ProjectNewCodingSessionDialog = React.lazy(async () => {
  const module = await import(
    "@/features/projects-container/ui/ProjectNewCodingSessionDialog"
  );
  return { default: module.ProjectNewCodingSessionDialog };
});

/**
 * The one place the create dialog is mounted.
 *
 * Mounted by the app shell rather than by each caller: a session can be
 * started from a channel menu, a project sidebar, or a keyboard shortcut, and
 * three copies of this dialog would be three chances for two of them to be
 * open at once.
 */
export function NewCodingSessionDialogHost() {
  const request = useNewCodingSessionRequest();
  if (request === null) return null;
  if (request.kind === "project") {
    return (
      // No fallback: a spinner behind a modal that has not appeared yet is
      // worse than the brief nothing before the chunk resolves.
      <React.Suspense fallback={null}>
        <ProjectNewCodingSessionDialog
          onOpenChange={(open) => {
            if (!open) closeNewCodingSessionDialog();
          }}
          open
          projectId={request.projectId}
        />
      </React.Suspense>
    );
  }
  return (
    <NewCodingSessionDialog
      channelId={request.channelId ?? undefined}
      onOpenChange={(open) => {
        if (!open) closeNewCodingSessionDialog();
      }}
      open
    />
  );
}
