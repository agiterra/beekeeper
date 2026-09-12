import * as React from "react";
import { Button } from "@/shared/ui/button";
import { Textarea } from "@/shared/ui/textarea";
import {
  projectTeamSetupBrief,
  projectTeamSetupError,
  type ProjectTeamSetupDraft,
  type ProjectTeamSetupSnapshot,
  type ProjectTeamSetupValidation,
} from "../lib/projectTeamSetup";
import {
  snapshotProjectTeamSetup,
  validateProjectTeamSetup,
} from "../lib/projectTeamSetupApi";

/** An authoring handoff; it conveys no authority to publish the draft. */
export type StartProjectTeamAuthoring = (
  draft: ProjectTeamSetupDraft,
  brief: string,
) => Promise<void>;

export function ProjectTeamSetupDraftView({
  draft,
  onStartAuthoring,
}: {
  draft: ProjectTeamSetupDraft;
  onStartAuthoring?: StartProjectTeamAuthoring;
}) {
  const [validation, setValidation] =
    React.useState<ProjectTeamSetupValidation | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [snapshot, setSnapshot] =
    React.useState<ProjectTeamSetupSnapshot | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  React.useEffect(() => {
    if (!draft.latestSnapshotId) return;
    let cancelled = false;
    setBusy(true);
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
        if (!cancelled) setError(projectTeamSetupError(failure));
      })
      .finally(() => {
        if (!cancelled) setBusy(false);
      });
    return () => {
      cancelled = true;
    };
  }, [draft.latestSnapshotId, draft.projectRef, draft.relayUrl, draft.setupId]);
  const brief = projectTeamSetupBrief(draft);
  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (failure) {
      setError(projectTeamSetupError(failure));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="space-y-4" data-testid="project-team-setup-draft">
      <div className="space-y-1">
        <h3 className="text-base font-semibold">Draft ready</h3>
        <p className="text-sm text-muted-foreground">
          Draft role packs are saved on this computer. Review and adapt them for
          your project in an authoring session.
        </p>
        <p className="text-sm" data-testid="project-team-setup-publication">
          This draft has not been published or applied to team sessions.
        </p>
      </div>
      <dl className="space-y-2 text-sm">
        <div>
          <dt className="font-medium">Project intent</dt>
          <dd className="whitespace-pre-wrap">{draft.intent}</dd>
        </div>
        <div>
          <dt className="font-medium">
            Repository folder (checked when prepared)
          </dt>
          <dd className="break-all">{draft.projectDirectory}</dd>
        </div>
        <div>
          <dt className="font-medium">Draft role packs</dt>
          <dd className="break-all">{draft.rolesDirectory}</dd>
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
      <div
        className="space-y-2 rounded-md border p-3"
        data-testid="project-team-setup-validation"
      >
        <h3 className="text-sm font-medium">Local validation</h3>
        <p className="text-sm text-muted-foreground">
          {validation
            ? validation.valid
              ? "Pack structure passed. Project suitability and execution use still need evidence."
              : "Pack structure needs corrections."
            : "The draft has not been checked yet."}
        </p>
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
                setValidation(
                  await validateProjectTeamSetup({
                    projectRef: draft.projectRef,
                    expectedRelayUrl: draft.relayUrl,
                    setupId: draft.setupId,
                  }),
                );
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
                  const saved = await snapshotProjectTeamSetup({
                    projectRef: draft.projectRef,
                    expectedRelayUrl: draft.relayUrl,
                    setupId: draft.setupId,
                  });
                  setSnapshot(saved);
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
          <div className="space-y-1 text-sm" role="status">
            <p>
              Checked version saved. Later draft edits do not change this copy.
              It has not been published.
            </p>
            <details>
              <summary className="cursor-pointer">
                Saved version details
              </summary>
              <p className="break-all">{snapshot.snapshotId}</p>
              <p>Roles: {snapshot.roles.join(", ")}</p>
            </details>
          </div>
        ) : null}
      </div>
      <details>
        <summary className="cursor-pointer text-sm font-medium">
          Setup brief for the authoring session
        </summary>
        <Textarea
          aria-label="Setup brief"
          className="mt-2 min-h-56 text-sm"
          readOnly
          value={brief}
        />
      </details>
      {onStartAuthoring ? (
        <Button
          disabled={busy}
          onClick={() =>
            void run(async () => {
              setValidation(null);
              await onStartAuthoring(draft, brief);
            })
          }
          type="button"
        >
          Start authoring session
        </Button>
      ) : (
        <p className="text-sm text-muted-foreground">
          Next: use the setup brief to adapt these draft packs in a project
          authoring session.
        </p>
      )}
      {error ? (
        <p className="text-sm text-destructive" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}
