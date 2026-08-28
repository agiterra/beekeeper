import { UserPlus, Users } from "lucide-react";
import * as React from "react";

import type { ProjectContainer } from "../hooks";
import { rosterWithOwner, useProjectRosterQuery } from "../lib/projectMembers";
import {
  ProjectMembersManager,
  useViewerIsProjectOwner,
} from "./ProjectMembersManager";
import { SectionCard } from "./SectionCard";

/**
 * The project page's Members card — a SectionCard shell around
 * `ProjectMembersManager`, which owns the roster list and the add/remove
 * flows (also mounted by the Project Settings dialog's Members tab).
 */
export function ProjectMembersCard({ project }: { project: ProjectContainer }) {
  const rosterQuery = useProjectRosterQuery(project);
  const roster = rosterQuery.data ?? project.members;
  const memberCount = React.useMemo(
    () => rosterWithOwner(project, roster).length,
    [project, roster],
  );
  const viewerIsOwner = useViewerIsProjectOwner(project);
  const [addOpen, setAddOpen] = React.useState(false);

  return (
    <SectionCard
      count={memberCount}
      icon={<Users className="size-4" />}
      testId="project-members-card"
      title="Members"
      action={
        viewerIsOwner ? (
          <button
            aria-label="Add members"
            className="flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground/70 transition-colors hover:bg-muted hover:text-foreground"
            data-testid="project-members-add"
            onClick={() => setAddOpen(true)}
            type="button"
          >
            <UserPlus className="size-4" />
          </button>
        ) : undefined
      }
    >
      <ProjectMembersManager
        addOpen={addOpen}
        onAddOpenChange={setAddOpen}
        project={project}
      />
    </SectionCard>
  );
}
