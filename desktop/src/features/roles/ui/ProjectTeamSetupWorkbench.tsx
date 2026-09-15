import * as React from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useCommunities } from "@/features/communities/useCommunities";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { projectTeamSetupBlocker } from "../lib/projectTeamSetup";
import { useProjectTeamSetupAgentDirectory } from "../lib/useProjectTeamSetupAgentDirectory";
import {
  projectTeamSetupSummaryQueryKey,
  useProjectTeamSetupSummaryQuery,
} from "../lib/useProjectTeamSetupSummary";
import { ProjectTeamSetupAgentsContext } from "./ProjectTeamSetupAgents";
import { ProjectTeamSetupOpenButton } from "./ProjectTeamSetupOpenButton";
import type { StartProjectTeamAuthoring } from "./ProjectTeamSetupDraftView";
import { ProjectTeamSetupForm } from "./ProjectTeamSetupForm";

export { ProjectTeamSetupForm } from "./ProjectTeamSetupForm";

const ProjectTeamSetupAuthoring = React.lazy(() =>
  import("./ProjectTeamSetupAuthoring").then((module) => ({
    default: module.ProjectTeamSetupAuthoring,
  })),
);

/** Project-scoped entry point; merely opening it changes no project setup. */
export function ProjectTeamSetupWorkbench({
  projectId,
  projectRef,
  projectName,
  onStartAuthoring,
}: {
  /** The route id, for the roster's link to the project's Agents tab. */
  projectId?: string;
  projectRef: string;
  projectName: string;
  onStartAuthoring?: StartProjectTeamAuthoring;
}) {
  const [open, setOpen] = React.useState(false);
  const { activeCommunity } = useCommunities();
  const relayUrl = activeCommunity?.relayUrl ?? "";
  const queryClient = useQueryClient();
  const unavailable = projectTeamSetupBlocker({
    projectRef,
    relayUrl,
    intent: "setup",
    projectDirectory: "selected",
  });
  const summary = useProjectTeamSetupSummaryQuery(
    projectRef,
    relayUrl,
    !unavailable,
  );
  const close = React.useCallback(() => {
    setOpen(false);
    // The dialog may have prepared, saved, published or installed; re-read.
    void queryClient.invalidateQueries({
      queryKey: projectTeamSetupSummaryQueryKey(projectRef, relayUrl),
    });
  }, [projectRef, queryClient, relayUrl]);
  const directory = useProjectTeamSetupAgentDirectory(open);
  const navigate = useNavigate();
  const agents = React.useMemo(
    () => ({
      ...directory,
      openAgentsTab: projectId
        ? () => {
            close();
            void navigate({
              to: "/projects/$projectId/agents",
              params: { projectId },
            });
          }
        : null,
      openLeadSession: ({
        channelId,
        sessionRef,
      }: {
        channelId: string;
        sessionRef: string;
      }) => {
        close();
        // The founded route hands off to the lead's execution once one exists.
        void navigate({
          to: "/coding-sessions/$channelId/founded/$sessionRef",
          params: { channelId, sessionRef },
        });
      },
    }),
    [close, directory, navigate, projectId],
  );
  const onOpenChange = (next: boolean) => {
    if (next) setOpen(true);
    else close();
  };
  return (
    <div>
      <ProjectTeamSetupOpenButton
        failed={summary.isError}
        onOpen={() => setOpen(true)}
        summary={summary.data}
        unavailable={unavailable}
      />
      <Dialog onOpenChange={onOpenChange} open={open}>
        <DialogContent
          className="max-h-[85vh] overflow-y-auto sm:max-w-2xl"
          data-testid="project-team-setup-dialog"
        >
          <DialogHeader>
            <DialogTitle>Set up {projectName}’s roles</DialogTitle>
            <DialogDescription>
              Prepare shared role instructions and skills grounded in this
              project's work.
            </DialogDescription>
          </DialogHeader>
          {open && !unavailable ? (
            <ProjectTeamSetupAgentsContext.Provider value={agents}>
              <ProjectTeamSetupForm
                key={`${projectRef}:${relayUrl}`}
                onStartAuthoring={onStartAuthoring}
                projectRef={projectRef}
                projectName={projectName}
                relayUrl={relayUrl}
                renderAuthoring={(
                  draft,
                  onDraftMayChange,
                  onLaunchObserved,
                ) => (
                  <React.Suspense
                    fallback={
                      <p className="text-sm">Loading authoring controls…</p>
                    }
                  >
                    <ProjectTeamSetupAuthoring
                      draft={draft}
                      onDraftMayChange={onDraftMayChange}
                      onLaunchObserved={onLaunchObserved}
                    />
                  </React.Suspense>
                )}
              />
            </ProjectTeamSetupAgentsContext.Provider>
          ) : null}
        </DialogContent>
      </Dialog>
    </div>
  );
}
