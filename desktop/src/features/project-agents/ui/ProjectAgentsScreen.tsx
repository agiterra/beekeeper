import * as React from "react";

import { useAppNavigation } from "@/app/navigation/useAppNavigation";
import { ProjectPageTabs } from "@/features/projects-container/ui/ProjectPageTabs";
import { useFeatureEnabled } from "@/shared/features";

import { useProjectAgents } from "../lib/useProjectAgents";
import type { OpenProjectAgentSession } from "./ProjectAgentRow";
import { ProjectAgentsView } from "./ProjectAgentsView";
import { PROJECT_AGENTS_MISSING } from "./projectAgentsCopy";

/**
 * `/projects/$projectId/agents` — who belongs to this project, with what
 * primary role, doing what; and who else took part. Replaces the Contributors
 * tab; its seat history is the Previously section.
 */
export function ProjectAgentsScreen({ projectId }: { projectId: string }) {
  const {
    project,
    model,
    isLoading,
    shelfState,
    assignments,
    readErrors,
    associateAccess,
  } = useProjectAgents(projectId);
  const pulseEnabled = useFeatureEnabled("project-pulse");
  const { goCodingSession, goFoundedCodingSession } = useAppNavigation();

  const onOpenSession = React.useCallback<OpenProjectAgentSession>(
    (target, sessionRef) => {
      if (target.founded && sessionRef) {
        void goFoundedCodingSession(target.channelId, sessionRef);
        return;
      }
      void goCodingSession(target.channelId, target.generationId);
    },
    [goCodingSession, goFoundedCodingSession],
  );

  const notices = React.useMemo(() => {
    const list = [...readErrors];
    if (shelfState.kind === "partial" || shelfState.kind === "unavailable") {
      list.push(`${shelfState.message} — ${shelfState.detail}`);
    }
    return list;
  }, [readErrors, shelfState]);

  if (!project) {
    return (
      <div
        className="flex flex-1 items-center justify-center p-4"
        data-testid="project-agents-missing"
      >
        <p className="text-sm text-muted-foreground">
          {PROJECT_AGENTS_MISSING}
        </p>
      </div>
    );
  }

  return (
    <div
      className="flex h-full min-h-0 min-w-0 flex-col overflow-y-auto p-4"
      data-testid="project-agents-screen"
    >
      <div>
        <h1 className="break-words text-xl font-semibold text-foreground">
          {project.name}
        </h1>
        <ProjectPageTabs
          active="agents"
          projectId={project.id}
          showPulse={pulseEnabled}
        />
      </div>
      <ProjectAgentsView
        assignments={assignments}
        associateAccess={associateAccess}
        isLoading={isLoading}
        model={model}
        notices={notices}
        onOpenSession={onOpenSession}
        projectId={project.id}
        projectName={project.name}
        projectRef={project.address}
      />
    </div>
  );
}
