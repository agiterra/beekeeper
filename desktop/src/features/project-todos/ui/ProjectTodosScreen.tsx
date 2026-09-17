import * as React from "react";
import { useNavigate } from "@tanstack/react-router";
import { toast } from "sonner";

import { useUsersBatchQuery } from "@/features/profile/hooks";
import { useProjectContainerQuery } from "@/features/projects-container/hooks";
import { useProjectRosterQuery } from "@/features/projects-container/lib/projectMembers";
import { useProjectCapabilities } from "@/features/projects-container/lib/projectPermissions";
import { LOCAL_GENERAL_ID } from "@/features/projects-container/lib/projectContainerModel";
import { ProjectPageTabs } from "@/features/projects-container/ui/ProjectPageTabs";
import { useIdentityQuery } from "@/shared/api/hooks";
import { useFeatureEnabled } from "@/shared/features";

import { todoWriteAccess } from "../lib/todoAccess";
import { useTodoMutations } from "../lib/todoMutations";
import { todoPerson, todoPubkeysOf } from "../lib/todoPeople";
import { useProjectTodos } from "../lib/todoQueries";
import { ProjectTodosView } from "./ProjectTodosView";

/**
 * `/projects/$projectId/todos` — the project's shared to-do lists: what is
 * left to do, who has it, when it is due, and what got done, live for every
 * member. Writes are one signed op per field (NIP-TD, kind 44248).
 */
export function ProjectTodosScreen({
  projectId,
  selectedListId,
  focused = false,
}: {
  projectId: string;
  /** The `?list=` the route carries, or null. */
  selectedListId: string | null;
  /** `?view=list`: show that one list alone, the way a sidebar row opens it. */
  focused?: boolean;
}) {
  const { project } = useProjectContainerQuery(projectId);
  const navigate = useNavigate();
  const onSelectList = React.useCallback(
    (listId: string | null) => {
      void navigate({
        to: "/projects/$projectId/todos",
        params: { projectId },
        search: listId ? { list: listId } : {},
        replace: true,
      });
    },
    [navigate, projectId],
  );
  const pulseEnabled = useFeatureEnabled("project-pulse");
  const coordinate =
    project && project.id !== LOCAL_GENERAL_ID && project.owner.length > 0
      ? project.address
      : null;
  const state = useProjectTodos(coordinate);
  const mutations = useTodoMutations(coordinate);
  const capabilities = useProjectCapabilities(project);
  const rosterQuery = useProjectRosterQuery(project);
  const identity = useIdentityQuery();
  const access = todoWriteAccess(
    identity.data?.pubkey ?? null,
    project,
    rosterQuery.data ?? project?.members ?? [],
    capabilities,
  );

  const pubkeys = React.useMemo(
    () => todoPubkeysOf(state.read?.digest.lists ?? []),
    [state.read],
  );
  const profiles = useUsersBatchQuery(pubkeys);
  const profileMap = profiles.data?.profiles;
  const personFor = React.useCallback(
    (pubkey: string) => todoPerson(pubkey, profileMap),
    [profileMap],
  );
  const onWriteError = React.useCallback((message: string) => {
    toast.error(message);
  }, []);

  if (!project) {
    return (
      <div
        className="flex flex-1 items-center justify-center p-4"
        data-testid="project-todos-missing"
      >
        <p className="text-sm text-muted-foreground">Project not found.</p>
      </div>
    );
  }

  return (
    <div
      className="flex h-full min-h-0 min-w-0 flex-col overflow-y-auto p-4"
      data-testid="project-todos-screen"
    >
      {focused && selectedListId ? null : (
        <div>
          <h1 className="break-words text-xl font-semibold text-foreground">
            {project.name}
          </h1>
          <ProjectPageTabs
            active="todos"
            projectId={project.id}
            showPulse={pulseEnabled}
          />
        </div>
      )}
      <ProjectTodosView
        access={access}
        focused={focused && selectedListId !== null}
        mutations={mutations}
        onSelectList={onSelectList}
        onWriteError={onWriteError}
        personFor={personFor}
        project={project}
        selectedListId={selectedListId}
        state={state}
      />
    </div>
  );
}
