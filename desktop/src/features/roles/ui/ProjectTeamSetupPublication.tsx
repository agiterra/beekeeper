import * as React from "react";
import { Button } from "@/shared/ui/button";
import {
  projectTeamSetupError,
  type ProjectTeamSetupActivation,
  type ProjectTeamSetupDraft,
  type ProjectTeamSetupPublication as ProjectTeamSetupPublicationState,
  type ProjectTeamSetupPublicationOptions,
  type ProjectTeamSetupSnapshot,
} from "../lib/projectTeamSetup";
import {
  continueProjectTeamSetupPublication,
  getProjectTeamSetupActivation,
  getProjectTeamSetupPublicationOptions,
  installProjectTeamSetupActivation,
  ensureProjectTeamSetupLeadChannel,
  startProjectTeamSetupLead,
  startProjectTeamSetupPublication,
} from "../lib/projectTeamSetupApi";

function publicationStatus(status: ProjectTeamSetupPublicationState["status"]) {
  switch (status) {
    case "checking":
      return "Checking the selected version";
    case "candidate_prepared":
      return "Candidate revision prepared; it has not been pushed or adopted";
    case "push_unknown":
      return "Repository push needs confirmation";
    case "pushed":
      return "Repository revision pushed; source adoption is still pending";
    case "source_unknown":
      return "Shared source adoption needs confirmation";
    case "adopted":
      return "Shared project source adopted";
    case "superseded":
      return "A later shared project source replaced this publication";
    case "conflict":
      return "Publication conflicted with the current project source";
    case "refused":
      return "Publication was refused";
  }
}

function needsContinuation(status: ProjectTeamSetupPublicationState["status"]) {
  return [
    "checking",
    "candidate_prepared",
    "push_unknown",
    "pushed",
    "source_unknown",
  ].includes(status);
}

function activationScope(draft: ProjectTeamSetupDraft, publicationId: string) {
  return {
    projectRef: draft.projectRef,
    expectedRelayUrl: draft.relayUrl,
    setupId: draft.setupId,
    publicationId,
  };
}

/**
 * Publishes exactly one saved snapshot through the host's journal. A saved
 * local copy remains distinct from a source that the relay has adopted.
 */
export function ProjectTeamSetupPublication({
  draft,
  snapshot,
}: {
  draft: ProjectTeamSetupDraft;
  snapshot: ProjectTeamSetupSnapshot;
  projectName?: string;
}) {
  const [options, setOptions] =
    React.useState<ProjectTeamSetupPublicationOptions | null>(null);
  const [publication, setPublication] =
    React.useState<ProjectTeamSetupPublicationState | null>(null);
  const [loadingOptions, setLoadingOptions] = React.useState(true);
  const [busy, setBusy] = React.useState(false);
  const [activation, setActivation] =
    React.useState<ProjectTeamSetupActivation | null>(null);
  const [creatingChannel, setCreatingChannel] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    let cancelled = false;
    setLoadingOptions(true);
    setError(null);
    setOptions(null);
    setPublication(null);
    void getProjectTeamSetupPublicationOptions({
      projectRef: draft.projectRef,
      expectedRelayUrl: draft.relayUrl,
      setupId: draft.setupId,
    })
      .then((next) => {
        if (cancelled) return;
        if (!next) {
          setError(
            "Publication choices are unavailable. Reopen setup and try again.",
          );
          return;
        }
        setOptions(next);
        setPublication(next.publication);
      })
      .catch((failure: unknown) => {
        if (!cancelled) setError(projectTeamSetupError(failure));
      })
      .finally(() => {
        if (!cancelled) setLoadingOptions(false);
      });
    return () => {
      cancelled = true;
    };
  }, [draft.projectRef, draft.relayUrl, draft.setupId]);

  React.useEffect(() => {
    if (publication?.status !== "adopted") {
      setActivation(null);
      return;
    }
    let cancelled = false;
    setError(null);
    void getProjectTeamSetupActivation(
      activationScope(draft, publication.publicationId),
    )
      .then((next) => {
        if (!cancelled) setActivation(next);
      })
      .catch((failure: unknown) => {
        if (!cancelled) setError(projectTeamSetupError(failure));
      });
    return () => {
      cancelled = true;
    };
  }, [draft, publication?.publicationId, publication?.status]);

  const publish = async () => {
    if (
      !options?.suggestedDestination ||
      (options.sourceExpectation.kind === "expected" &&
        !options.suggestedDestination.baseCommit)
    )
      return;
    setBusy(true);
    setError(null);
    try {
      setPublication(
        await startProjectTeamSetupPublication({
          projectRef: draft.projectRef,
          expectedRelayUrl: draft.relayUrl,
          setupId: draft.setupId,
          destination: options.suggestedDestination,
          sourceExpectation: options.sourceExpectation,
          output: { kind: "snapshot", snapshotId: snapshot.snapshotId },
        }),
      );
    } catch (failure) {
      setError(projectTeamSetupError(failure));
    } finally {
      setBusy(false);
    }
  };

  const continuePublication = async () => {
    if (!publication) return;
    setBusy(true);
    setError(null);
    try {
      setPublication(
        await continueProjectTeamSetupPublication({
          projectRef: draft.projectRef,
          expectedRelayUrl: draft.relayUrl,
          setupId: draft.setupId,
          publicationId: publication.publicationId,
        }),
      );
    } catch (failure) {
      setError(projectTeamSetupError(failure));
    } finally {
      setBusy(false);
    }
  };

  const install = async () => {
    if (!publication) return;
    setBusy(true);
    setError(null);
    try {
      setActivation(
        await installProjectTeamSetupActivation(
          activationScope(draft, publication.publicationId),
        ),
      );
    } catch (failure) {
      setError(projectTeamSetupError(failure));
    } finally {
      setBusy(false);
    }
  };

  const startLead = async (channelId = activation?.lead.channelId) => {
    if (!publication || !channelId) return;
    setBusy(true);
    setError(null);
    try {
      setActivation(
        await startProjectTeamSetupLead({
          ...activationScope(draft, publication.publicationId),
          channelId,
        }),
      );
    } catch (failure) {
      setError(projectTeamSetupError(failure));
    } finally {
      setBusy(false);
    }
  };

  const createSessionChannel = async () => {
    if (!publication || creatingChannel) return;
    setCreatingChannel(true);
    setError(null);
    try {
      setActivation(
        await ensureProjectTeamSetupLeadChannel(
          activationScope(draft, publication.publicationId),
        ),
      );
    } catch (failure) {
      setError(projectTeamSetupError(failure));
    } finally {
      setCreatingChannel(false);
    }
  };

  const blockedByMissingDestination =
    !publication && !options?.suggestedDestination;
  const blockedByMissingImmutableBase =
    !publication &&
    options?.sourceExpectation.kind === "expected" &&
    !options.suggestedDestination?.baseCommit;

  return (
    <section
      className="space-y-2 rounded-md border p-3"
      data-testid="project-team-setup-publication-stage"
    >
      <h3 className="text-sm font-medium">Publish checked version</h3>
      <p className="text-sm text-muted-foreground">
        Publish saved version {snapshot.snapshotId}. This authorizes these exact
        checked bytes; it does not install roles or start a lead session.
      </p>
      {loadingOptions ? (
        <p className="text-sm text-muted-foreground">
          Checking the project source before publication…
        </p>
      ) : blockedByMissingDestination ? (
        <p className="text-sm text-destructive" role="alert">
          The host could not identify a project destination from the current
          source. Publication is blocked so existing project procedures are not
          replaced.
        </p>
      ) : blockedByMissingImmutableBase ? (
        <p className="text-sm text-destructive" role="alert">
          The current project source has no immutable base revision. Refresh the
          project source before replacing its procedures.
        </p>
      ) : options ? (
        <div className="space-y-2">
          <p
            className="text-sm"
            data-testid="project-team-setup-publication-target"
          >
            Shared destination: {options.suggestedDestination?.repoRef} at{" "}
            {options.suggestedDestination?.packPath}
          </p>
          {!publication ? (
            <Button
              disabled={busy}
              onClick={() => void publish()}
              type="button"
            >
              {busy ? "Publishing…" : "Publish checked version"}
            </Button>
          ) : (
            <div className="space-y-2" role="status">
              <p>{publicationStatus(publication.status)}</p>
              {publication.message ? <p>{publication.message}</p> : null}
              {publication.status === "adopted" ? (
                activation?.source ? (
                  <p data-testid="project-team-setup-adopted-source">
                    Adopted source: {activation.source.repoRef} @{" "}
                    {activation.source.commit} / {activation.source.packPath}
                  </p>
                ) : (
                  <p className="text-sm text-muted-foreground">
                    Resolving the adopted source on this computer…
                  </p>
                )
              ) : null}
              {needsContinuation(publication.status) ? (
                <Button
                  disabled={busy}
                  onClick={() => void continuePublication()}
                  type="button"
                  variant="outline"
                >
                  {busy ? "Retrying…" : "Retry publication"}
                </Button>
              ) : null}
            </div>
          )}
        </div>
      ) : null}
      {publication?.status === "adopted" && activation ? (
        <section
          className="space-y-2 rounded-md border p-3"
          data-testid="project-team-setup-activation-stage"
        >
          <h3 className="text-sm font-medium">Install and start lead</h3>
          {activation.source ? (
            <p className="text-sm text-muted-foreground">
              Shared source: {activation.source.repoRef} @{" "}
              {activation.source.commit} / {activation.source.packPath}
            </p>
          ) : (
            <p className="text-sm text-destructive" role="alert">
              The adopted source could not be resolved on this device. Refresh
              publication before installing roles.
            </p>
          )}
          {activation.source &&
          activation.installation.status === "installed" ? (
            <p data-testid="project-team-setup-installed-roles">
              Roles available on this computer:{" "}
              {activation.installation.installedRoles
                .map((role) => role.role)
                .join(", ") || "none reported"}
            </p>
          ) : activation.source ? (
            <Button
              disabled={busy}
              onClick={() => void install()}
              type="button"
            >
              {busy
                ? "Installing…"
                : activation.installation.status === "unknown"
                  ? "Retry installation"
                  : "Install project roles"}
            </Button>
          ) : null}
          {activation.installation.message ? (
            <p className="text-sm text-muted-foreground">
              {activation.installation.message}
            </p>
          ) : null}
          {activation.source &&
          activation.installation.status === "installed" ? (
            <div
              className="space-y-2"
              data-testid="project-team-setup-lead-handoff"
            >
              {activation.lead.status === "needs_channel" ||
              (activation.lead.status === "unknown" &&
                !activation.lead.sessionRef) ? (
                <Button
                  disabled={creatingChannel || busy}
                  onClick={() => void createSessionChannel()}
                  type="button"
                  variant="outline"
                >
                  {creatingChannel
                    ? "Preparing project session channel…"
                    : activation.lead.channelId
                      ? "Retry session-channel setup"
                      : "Create project session channel"}
                </Button>
              ) : activation.lead.status === "started" ? (
                <p data-testid="project-team-setup-lead-started">
                  Project lead started in {activation.lead.channelId} (session{" "}
                  {activation.lead.sessionRef}).
                </p>
              ) : activation.lead.status === "starting" ? (
                <p className="text-sm text-muted-foreground" role="status">
                  Project lead handoff is in progress. Reopen this setup to
                  check the recorded outcome.
                </p>
              ) : activation.lead.status === "refused" ? null : (
                <Button
                  disabled={busy || !activation.lead.channelId}
                  onClick={() => void startLead()}
                  type="button"
                >
                  {busy
                    ? "Starting lead…"
                    : activation.lead.status === "unknown"
                      ? "Retry lead handoff"
                      : "Start project lead"}
                </Button>
              )}
              {activation.lead.message ? (
                <p className="text-sm text-muted-foreground">
                  {activation.lead.message}
                </p>
              ) : null}
            </div>
          ) : null}
        </section>
      ) : null}
      {error ? (
        <p className="text-sm text-destructive" role="alert">
          {error}
        </p>
      ) : null}
    </section>
  );
}
