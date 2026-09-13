import * as React from "react";
import {
  getCodingSessionWorkdirState,
  pickCodingSessionWorkdir,
} from "@/shared/api/tauriCodingSessionWorkdirs";
import { Button } from "@/shared/ui/button";
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
  projectName,
  onStartAuthoring,
  renderAuthoring,
}: {
  projectRef: string;
  relayUrl: string;
  projectName?: string;
  onStartAuthoring?: StartProjectTeamAuthoring;
  renderAuthoring?: (
    draft: ProjectTeamSetupDraft,
    onDraftMayChange: () => void,
  ) => React.ReactNode;
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
        projectName={projectName}
        authoring={
          renderAuthoring
            ? (onDraftMayChange) => renderAuthoring(draft, onDraftMayChange)
            : undefined
        }
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
