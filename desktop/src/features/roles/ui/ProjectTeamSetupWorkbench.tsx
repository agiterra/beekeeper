import * as React from "react";
import { useCommunities } from "@/features/communities/useCommunities";
import {
  getCodingSessionWorkdirState,
  pickCodingSessionWorkdir,
} from "@/shared/api/tauriCodingSessionWorkdirs";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";
import { Textarea } from "@/shared/ui/textarea";
import {
  projectTeamSetupBlocker,
  projectTeamSetupError,
  type ProjectTeamSetupDraft,
} from "../lib/projectTeamSetup";
import {
  getProjectTeamSetup,
  prepareProjectTeamSetup,
} from "../lib/projectTeamSetupApi";
import {
  ProjectTeamSetupDraftView,
  type StartProjectTeamAuthoring,
} from "./ProjectTeamSetupDraftView";

/** Mounted only while open. Initial reads never prepare or publish a draft. */
export function ProjectTeamSetupForm({
  projectRef,
  relayUrl,
  onStartAuthoring,
}: {
  projectRef: string;
  relayUrl: string;
  onStartAuthoring?: StartProjectTeamAuthoring;
}) {
  const [intent, setIntent] = React.useState("");
  const [directory, setDirectory] = React.useState("");
  const [draft, setDraft] = React.useState<ProjectTeamSetupDraft | null>(null);
  const [loading, setLoading] = React.useState(true);
  const [readFailed, setReadFailed] = React.useState(false);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [reload, setReload] = React.useState(0);
  const directoryTouched = React.useRef(false);
  // biome-ignore lint/correctness/useExhaustiveDependencies: reload is the explicit retry trigger after a failed read.
  React.useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setReadFailed(false);
    setError(null);
    void Promise.allSettled([
      getProjectTeamSetup({ projectRef, expectedRelayUrl: relayUrl }),
      getCodingSessionWorkdirState(),
    ]).then(([saved, workdirs]) => {
      if (cancelled) return;
      if (saved.status === "fulfilled") setDraft(saved.value);
      else {
        setReadFailed(true);
        setError(projectTeamSetupError(saved.reason));
      }
      if (workdirs.status === "fulfilled" && !directoryTouched.current) {
        setDirectory(workdirs.value.byProject[projectRef]?.path ?? "");
      }
      setLoading(false);
    });
    return () => {
      cancelled = true;
    };
  }, [projectRef, relayUrl, reload]);

  const blocker = projectTeamSetupBlocker({
    projectRef,
    relayUrl,
    intent,
    projectDirectory: directory,
  });
  const prepare = async () => {
    if (blocker || busy || loading || readFailed) return;
    setBusy(true);
    setError(null);
    try {
      setDraft(
        await prepareProjectTeamSetup({
          projectRef,
          expectedRelayUrl: relayUrl,
          intent: intent.trim(),
          projectDirectory: directory.trim(),
        }),
      );
    } catch (failure) {
      setError(projectTeamSetupError(failure));
    } finally {
      setBusy(false);
    }
  };
  if (loading)
    return (
      <p className="text-sm" role="status">
        Checking for a saved draft…
      </p>
    );
  if (draft)
    return (
      <ProjectTeamSetupDraftView
        draft={draft}
        key={draft.setupId}
        onStartAuthoring={onStartAuthoring}
      />
    );
  return (
    <div className="space-y-4" data-testid="project-team-setup-form">
      <div className="space-y-2">
        <label
          className="text-sm font-medium"
          htmlFor="project-team-setup-intent"
        >
          What should this project accomplish?
        </label>
        <Textarea
          disabled={busy}
          id="project-team-setup-intent"
          onChange={(event) => setIntent(event.target.value)}
          placeholder="Describe the product and the work you want the team to handle."
          value={intent}
        />
      </div>
      <div className="space-y-2">
        <label
          className="text-sm font-medium"
          htmlFor="project-team-setup-directory"
        >
          Local project repository
        </label>
        <div className="flex gap-2">
          <Input
            disabled={busy}
            id="project-team-setup-directory"
            onChange={(event) => {
              directoryTouched.current = true;
              setDirectory(event.target.value);
            }}
            value={directory}
          />
          <Button
            disabled={busy}
            onClick={() => {
              void pickCodingSessionWorkdir()
                .then((path) => {
                  if (path) {
                    directoryTouched.current = true;
                    setDirectory(path);
                  }
                })
                .catch((failure: unknown) =>
                  setError(projectTeamSetupError(failure)),
                );
            }}
            type="button"
            variant="outline"
          >
            Browse
          </Button>
        </div>
        <p className="text-xs text-muted-foreground">
          Prepare checks that this folder is the Git repository root and creates
          a separate local draft.
        </p>
      </div>
      {error ? (
        <p className="text-sm text-destructive" role="alert">
          {error}
        </p>
      ) : null}
      {readFailed ? (
        <Button
          onClick={() => setReload((value) => value + 1)}
          type="button"
          variant="outline"
        >
          Check saved draft again
        </Button>
      ) : null}
      <p className="text-sm text-muted-foreground">
        This new baseline draft starts from neutral role packs. Your existing
        project pack source stays unchanged; this does not edit published packs.
      </p>
      {blocker ? (
        <p className="text-sm text-muted-foreground">{blocker}</p>
      ) : null}
      <Button
        disabled={Boolean(blocker) || busy || readFailed}
        onClick={() => void prepare()}
        type="button"
      >
        {busy ? "Preparing…" : "Prepare draft"}
      </Button>
    </div>
  );
}

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
            />
          ) : null}
        </DialogContent>
      </Dialog>
    </div>
  );
}
