import * as React from "react";

import {
  closeNewCodingSessionDialog,
  useNewCodingSessionRequest,
  type NewCodingSessionRequest,
} from "../newCodingSessionDialogStore";
import {
  NewCodingSessionDialog,
  type NewCodingSessionWorkspaceReuse,
} from "./NewCodingSessionDialog";

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

/** Which dialog a request opens, and what it carries there. */
export type NewCodingSessionDialogRoute =
  | {
      kind: "project";
      projectId: string;
      workspaceReuse: NewCodingSessionWorkspaceReuse | null;
    }
  | {
      kind: "channel";
      channelId: string | undefined;
      workspaceReuse: NewCodingSessionWorkspaceReuse | null;
    };

/**
 * A workspace request that names a project goes **through** the project
 * wrapper, not around it.
 *
 * The wrapper is where a project's coordinate and repository binding come
 * from — it resolves `projectRef` from the project's address and `repoRef`
 * from the checkout it matched, and the create signs both. Opening the plain
 * dialog for a project session because its directory was already chosen would
 * create a session with no project placement and no repo binding: a session
 * that runs in the right folder and belongs nowhere.
 *
 * Reusing a directory answers where a session *runs*. It never answers which
 * project it belongs to, so it never changes which dialog opens.
 */
export function resolveNewCodingSessionDialogRoute(
  request: NewCodingSessionRequest,
): NewCodingSessionDialogRoute {
  if (request.kind === "project") {
    return {
      kind: "project",
      projectId: request.projectId,
      workspaceReuse: null,
    };
  }
  if (request.kind === "workspace") {
    if (request.projectId !== null && request.projectId.length > 0) {
      return {
        kind: "project",
        projectId: request.projectId,
        workspaceReuse: request.workspace,
      };
    }
    return {
      kind: "channel",
      channelId: request.channelId ?? undefined,
      workspaceReuse: request.workspace,
    };
  }
  return {
    kind: "channel",
    channelId: request.channelId ?? undefined,
    workspaceReuse: null,
  };
}

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
  const route = resolveNewCodingSessionDialogRoute(request);
  const onOpenChange = (open: boolean) => {
    if (!open) closeNewCodingSessionDialog();
  };
  if (route.kind === "project") {
    return (
      // No fallback: a spinner behind a modal that has not appeared yet is
      // worse than the brief nothing before the chunk resolves.
      <React.Suspense fallback={null}>
        <ProjectNewCodingSessionDialog
          onOpenChange={onOpenChange}
          open
          projectId={route.projectId}
          workspaceReuse={route.workspaceReuse}
        />
      </React.Suspense>
    );
  }
  return (
    <NewCodingSessionDialog
      channelId={route.channelId}
      onOpenChange={onOpenChange}
      open
      workspaceReuse={route.workspaceReuse}
    />
  );
}
