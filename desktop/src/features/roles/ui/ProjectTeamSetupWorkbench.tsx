import * as React from "react";
import { useCommunities } from "@/features/communities/useCommunities";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { projectTeamSetupBlocker } from "../lib/projectTeamSetup";
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
  projectRef,
  projectName,
  onStartAuthoring,
}: {
  projectRef: string;
  projectName: string;
  onStartAuthoring?: StartProjectTeamAuthoring;
}) {
  const [open, setOpen] = React.useState(false);
  const { activeCommunity } = useCommunities();
  const relayUrl = activeCommunity?.relayUrl ?? "";
  const unavailable = projectTeamSetupBlocker({
    projectRef,
    relayUrl,
    intent: "setup",
    projectDirectory: "selected",
  });
  return (
    <div>
      <Button
        data-testid="project-team-setup-open"
        disabled={Boolean(unavailable)}
        onClick={() => setOpen(true)}
        type="button"
        variant="outline"
      >
        Set up project team
      </Button>
      {unavailable ? (
        <p className="mt-1 text-sm text-muted-foreground">{unavailable}</p>
      ) : null}
      <Dialog onOpenChange={setOpen} open={open}>
        <DialogContent
          className="max-h-[85vh] overflow-y-auto sm:max-w-2xl"
          data-testid="project-team-setup-dialog"
        >
          <DialogHeader>
            <DialogTitle>Set up {projectName}’s team</DialogTitle>
            <DialogDescription>
              Prepare shared roles and skills grounded in this project's work.
            </DialogDescription>
          </DialogHeader>
          {open && !unavailable ? (
            <ProjectTeamSetupForm
              key={`${projectRef}:${relayUrl}`}
              onStartAuthoring={onStartAuthoring}
              projectRef={projectRef}
              relayUrl={relayUrl}
              renderAuthoring={(draft, onDraftMayChange) => (
                <React.Suspense
                  fallback={
                    <p className="text-sm">Loading authoring controls…</p>
                  }
                >
                  <ProjectTeamSetupAuthoring
                    draft={draft}
                    onDraftMayChange={onDraftMayChange}
                  />
                </React.Suspense>
              )}
            />
          ) : null}
        </DialogContent>
      </Dialog>
    </div>
  );
}
