import * as React from "react";
import { Button } from "@/shared/ui/button";
import { Textarea } from "@/shared/ui/textarea";
import {
  projectTeamSetupFailure,
  type ProjectTeamSetupDraft,
  type ProjectTeamSetupFailure,
  type ProjectTeamSetupLaunch,
  type ProjectTeamSetupSnapshot,
  type ProjectTeamSetupValidation,
} from "../lib/projectTeamSetup";
import {
  getProjectTeamSetupBrief,
  snapshotProjectTeamSetup,
  validateProjectTeamSetup,
} from "../lib/projectTeamSetupApi";
import {
  projectTeamSetupStage,
  projectTeamSetupStepIndex,
} from "../lib/projectTeamSetupStage";
import { ProjectTeamSetupFailureNotice } from "./ProjectTeamSetupFailureNotice";
import {
  ProjectTeamSetupPublication,
  type ProjectTeamSetupPublicationProgress,
} from "./ProjectTeamSetupPublication";
import {
  ProjectTeamSetupSentence,
  useProjectTeamSetupRoster,
} from "./ProjectTeamSetupRoster";
import { ProjectTeamSetupStepper } from "./ProjectTeamSetupStepper";
import { ProjectTeamSetupTechnicalDetails } from "./ProjectTeamSetupTechnicalDetails";

/** An authoring handoff; it conveys no authority to publish the draft. */
export type StartProjectTeamAuthoring = (
  draft: ProjectTeamSetupDraft,
  brief: string,
) => Promise<void>;

/**
 * What the authoring controls observed: the saved launch, `null` for none,
 * or `"unreadable"` when the saved records could not be read.
 */
export type ProjectTeamSetupLaunchObservation =
  | ProjectTeamSetupLaunch
  | null
  | "unreadable";

export type RenderProjectTeamSetupAuthoring = (
  onDraftMayChange: () => void,
  onLaunchObserved: (launch: ProjectTeamSetupLaunchObservation) => void,
) => React.ReactNode;

export function ProjectTeamSetupDraftView({
  draft,
  onStartAuthoring,
  authoring,
  projectName,
}: {
  draft: ProjectTeamSetupDraft;
  onStartAuthoring?: StartProjectTeamAuthoring;
  authoring?: RenderProjectTeamSetupAuthoring;
  projectName?: string;
}) {
  const [validation, setValidation] =
    React.useState<ProjectTeamSetupValidation | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [verifying, setVerifying] = React.useState(
    Boolean(draft.latestSnapshotId),
  );
  const [snapshot, setSnapshot] =
    React.useState<ProjectTeamSetupSnapshot | null>(null);
  const [verifyFailed, setVerifyFailed] = React.useState(false);
  const [launch, setLaunch] = React.useState<
    ProjectTeamSetupLaunchObservation | undefined
  >(undefined);
  const [progress, setProgress] =
    React.useState<ProjectTeamSetupPublicationProgress | null>(null);
  const [showCheck, setShowCheck] = React.useState(false);
  const [brief, setBrief] = React.useState<string | null>(null);
  const [briefError, setBriefError] =
    React.useState<ProjectTeamSetupFailure | null>(null);
  const [error, setError] = React.useState<ProjectTeamSetupFailure | null>(
    null,
  );
  React.useEffect(() => {
    if (!draft.latestSnapshotId) return;
    let cancelled = false;
    setBusy(true);
    setVerifying(true);
    void snapshotProjectTeamSetup({
      projectRef: draft.projectRef,
      expectedRelayUrl: draft.relayUrl,
      setupId: draft.setupId,
      snapshotId: draft.latestSnapshotId,
    })
      .then((saved) => {
        if (!cancelled) setSnapshot(saved);
      })
      .catch((failure: unknown) => {
        if (cancelled) return;
        setVerifyFailed(true);
        setError(projectTeamSetupFailure(failure));
      })
      .finally(() => {
        if (cancelled) return;
        setBusy(false);
        setVerifying(false);
      });
    return () => {
      cancelled = true;
    };
  }, [draft.latestSnapshotId, draft.projectRef, draft.relayUrl, draft.setupId]);
  const scope = {
    projectRef: draft.projectRef,
    expectedRelayUrl: draft.relayUrl,
    setupId: draft.setupId,
  };
  const loadBrief = async () => {
    if (brief !== null) return brief;
    setBriefError(null);
    try {
      const { text } = await getProjectTeamSetupBrief(scope);
      setBrief(text);
      return text;
    } catch (failure) {
      setBriefError(projectTeamSetupFailure(failure));
      throw failure;
    }
  };
  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (failure) {
      setError(projectTeamSetupFailure(failure));
    } finally {
      setBusy(false);
    }
  };
  const roster = useProjectTeamSetupRoster(
    draft.projectRef,
    progress?.activation,
  );
  const stage = projectTeamSetupStage({
    authoringLaunch: authoring ? launch : null,
    validation,
    snapshot: verifying
      ? "checking"
      : snapshot
        ? "saved"
        : verifyFailed
          ? "unverified"
          : "none",
    publication: snapshot
      ? progress && !progress.loading
        ? progress.publication
        : undefined
      : null,
    publicationBlocked: snapshot ? (progress?.blocked ?? null) : null,
    activation: progress?.activation,
    roster: roster ?? undefined,
    projectName,
  });
  const showValidation =
    !authoring ||
    showCheck ||
    validation !== null ||
    snapshot !== null ||
    projectTeamSetupStepIndex(stage.step) >= 1;
  return (
    <div className="space-y-4" data-testid="project-team-setup-draft">
      <div className="space-y-2">
        <h3
          className="text-base font-semibold"
          data-stage={stage.id}
          data-testid="project-team-setup-stage-heading"
        >
          {stage.title}
        </h3>
        <ProjectTeamSetupStepper stage={stage} />
        <p
          className="break-words text-sm"
          data-state={stage.state}
          data-testid="project-team-setup-next-action"
        >
          <ProjectTeamSetupSentence text={stage.next} />
        </p>
        <p
          className="text-sm text-muted-foreground"
          data-testid="project-team-setup-publication"
        >
          Draft edits stay local until you publish them.
        </p>
      </div>
      <dl className="space-y-2 text-sm">
        <div>
          <dt className="font-medium">Project intent</dt>
          <dd className="whitespace-pre-wrap">{draft.intent}</dd>
        </div>
        <div>
          <dt className="font-medium">
            {validation?.valid ? "Roles checked" : "Starting roles"}
          </dt>
          <dd>
            {(validation?.valid ? validation.roles : draft.roles).join(", ") ||
              "No roles recorded"}
          </dd>
        </div>
      </dl>
      {authoring?.(() => setValidation(null), setLaunch) ??
        (onStartAuthoring ? (
          <Button
            disabled={busy}
            onClick={() =>
              void run(async () => {
                setValidation(null);
                await onStartAuthoring(draft, await loadBrief());
              })
            }
            type="button"
          >
            Start authoring session
          </Button>
        ) : (
          <p className="text-sm text-muted-foreground">
            Use the setup brief to adapt these draft roles in a project
            authoring session, then check the draft.
          </p>
        ))}
      <details
        onToggle={(event) => {
          if (event.currentTarget.open) void loadBrief().catch(() => {});
        }}
      >
        <summary className="cursor-pointer text-sm font-medium">
          Setup brief sent to the authoring session
        </summary>
        {brief !== null ? (
          <Textarea
            aria-label="Setup brief"
            className="mt-2 min-h-56 text-sm"
            readOnly
            value={brief}
          />
        ) : briefError ? (
          <ProjectTeamSetupFailureNotice failure={briefError} />
        ) : (
          <p className="mt-2 text-sm text-muted-foreground">
            Reading the brief…
          </p>
        )}
      </details>
      {showValidation ? (
        <div
          className="space-y-2 rounded-md border p-3"
          data-testid="project-team-setup-validation"
        >
          <h3 className="text-sm font-medium">Check and save the draft</h3>
          <p className="text-sm text-muted-foreground">
            {validation
              ? validation.valid
                ? "Pack structure passed. Project suitability and execution use still need evidence."
                : "Pack structure needs corrections."
              : "The draft has not been checked yet."}
          </p>
          {snapshot ? (
            <p className="text-sm text-muted-foreground">
              Edited the draft since saving? Check it again, then save a new
              version.
            </p>
          ) : null}
          {validation?.diagnostics.length ? (
            <ul className="list-disc space-y-1 pl-5 text-sm">
              {validation.diagnostics.map((diagnostic) => (
                <li key={`${diagnostic.level}-${diagnostic.message}`}>
                  {diagnostic.level}: {diagnostic.message}
                </li>
              ))}
            </ul>
          ) : null}
          <div className="flex flex-wrap gap-2">
            <Button
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  setValidation(null);
                  setValidation(await validateProjectTeamSetup(scope));
                })
              }
              type="button"
              variant="outline"
            >
              {busy ? "Working…" : "Check draft"}
            </Button>
            {validation?.valid ? (
              <Button
                className="max-w-full whitespace-normal"
                disabled={busy}
                onClick={() =>
                  void run(async () => {
                    setSnapshot(null);
                    setProgress(null);
                    setSnapshot(await snapshotProjectTeamSetup(scope));
                  })
                }
                type="button"
                variant="outline"
              >
                Save checked version
              </Button>
            ) : null}
          </div>
          {snapshot ? (
            <p className="text-sm" role="status">
              Checked version saved. Later draft edits do not change this copy.
            </p>
          ) : null}
        </div>
      ) : (
        <Button
          onClick={() => setShowCheck(true)}
          size="sm"
          type="button"
          variant="ghost"
        >
          Edited the draft yourself? Check it now
        </Button>
      )}
      {snapshot ? (
        <ProjectTeamSetupPublication
          draft={draft}
          onProgress={setProgress}
          projectName={projectName}
          snapshot={snapshot}
        />
      ) : null}
      <ProjectTeamSetupTechnicalDetails
        draft={draft}
        progress={snapshot ? progress : null}
        snapshot={snapshot}
      />
      <ProjectTeamSetupFailureNotice failure={error} />
    </div>
  );
}
