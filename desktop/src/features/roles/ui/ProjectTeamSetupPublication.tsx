import * as React from "react";
import { Button } from "@/shared/ui/button";
import {
  projectTeamSetupFailure,
  type ProjectTeamSetupActivation,
  type ProjectTeamSetupFailure,
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
import { useProjectTeamSetupAgents } from "./ProjectTeamSetupAgents";
import { ProjectTeamSetupFailureNotice } from "./ProjectTeamSetupFailureNotice";
import { ProjectTeamSetupRoster } from "./ProjectTeamSetupRoster";

/**
 * What this card has observed, lifted so the draft view can derive one stage
 * and show identifiers under Technical details. `activation` is `undefined`
 * until read and `null` when the read failed.
 */
export type ProjectTeamSetupPublicationProgress = {
  loading: boolean;
  options: ProjectTeamSetupPublicationOptions | null;
  publication: ProjectTeamSetupPublicationState | null;
  blocked: "missing_destination" | "missing_base" | "unavailable" | null;
  activation: ProjectTeamSetupActivation | null | undefined;
};

function publicationStatus(status: ProjectTeamSetupPublicationState["status"]) {
  switch (status) {
    case "checking":
      return "Checking the selected version";
    case "candidate_prepared":
      return "Revision prepared; it has not been pushed or adopted";
    case "push_unknown":
      return "Repository push needs confirmation";
    case "pushed":
      return "Repository revision pushed; the project hasn't adopted it yet";
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
  projectName,
  onProgress,
}: {
  draft: ProjectTeamSetupDraft;
  snapshot: ProjectTeamSetupSnapshot;
  projectName?: string;
  onProgress?: (progress: ProjectTeamSetupPublicationProgress) => void;
}) {
  const [options, setOptions] =
    React.useState<ProjectTeamSetupPublicationOptions | null>(null);
  const [publication, setPublication] =
    React.useState<ProjectTeamSetupPublicationState | null>(null);
  const [loadingOptions, setLoadingOptions] = React.useState(true);
  const [busy, setBusy] = React.useState(false);
  const [activation, setActivation] =
    React.useState<ProjectTeamSetupActivation | null>(null);
  const [activationReadFailed, setActivationReadFailed] = React.useState(false);
  const [creatingChannel, setCreatingChannel] = React.useState(false);
  const [error, setError] = React.useState<ProjectTeamSetupFailure | null>(
    null,
  );
  const { refreshAgents } = useProjectTeamSetupAgents();

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
          setError({
            summary:
              "Publication choices are unavailable. Reopen setup and try again.",
            detail: null,
            code: null,
          });
          return;
        }
        setOptions(next);
        setPublication(next.publication);
      })
      .catch((failure: unknown) => {
        if (!cancelled) setError(projectTeamSetupFailure(failure));
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
    setActivationReadFailed(false);
    void getProjectTeamSetupActivation(
      activationScope(draft, publication.publicationId),
    )
      .then((next) => {
        if (!cancelled) setActivation(next);
      })
      .catch((failure: unknown) => {
        if (cancelled) return;
        setActivationReadFailed(true);
        setError(projectTeamSetupFailure(failure));
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
      setError(projectTeamSetupFailure(failure));
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
      setError(projectTeamSetupFailure(failure));
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
      setError(projectTeamSetupFailure(failure));
    } finally {
      setBusy(false);
      // Installation records each agent's project association on its local
      // record; re-read so the roster reflects what the host wrote.
      refreshAgents?.();
    }
  };

  /**
   * The one "Start the project lead" action: when no session channel is
   * recorded yet it is prepared first (same saved request), then the lead
   * handoff runs on the channel the host returned.
   */
  const startLead = async () => {
    if (!publication || !activation) return;
    setBusy(true);
    setError(null);
    try {
      let current = activation;
      if (current.lead.status === "needs_channel" || !current.lead.channelId) {
        current = await ensureProjectTeamSetupLeadChannel(
          activationScope(draft, publication.publicationId),
        );
        setActivation(current);
      }
      const channelId = current.lead.channelId;
      if (
        !channelId ||
        (current.lead.status !== "ready" && current.lead.status !== "unknown")
      )
        return;
      setActivation(
        await startProjectTeamSetupLead({
          ...activationScope(draft, publication.publicationId),
          channelId,
        }),
      );
    } catch (failure) {
      setError(projectTeamSetupFailure(failure));
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
      setError(projectTeamSetupFailure(failure));
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
  const blocked: ProjectTeamSetupPublicationProgress["blocked"] = loadingOptions
    ? null
    : !options
      ? "unavailable"
      : blockedByMissingDestination
        ? "missing_destination"
        : blockedByMissingImmutableBase
          ? "missing_base"
          : null;
  const reportedActivation =
    publication?.status !== "adopted"
      ? null
      : (activation ?? (activationReadFailed ? null : undefined));
  // biome-ignore lint/correctness/useExhaustiveDependencies: onProgress is a parent callback; progress is reported when observed facts change.
  React.useEffect(() => {
    onProgress?.({
      loading: loadingOptions,
      options,
      publication,
      blocked,
      activation: reportedActivation,
    });
  }, [loadingOptions, options, publication, blocked, reportedActivation]);

  return (
    <section
      className="space-y-2 rounded-md border p-3"
      data-testid="project-team-setup-publication-stage"
    >
      <h3 className="text-sm font-medium">Publish checked version</h3>
      <p className="text-sm text-muted-foreground">
        Publishing shares exactly the saved version with the project. It does
        not install roles or start a lead session.
      </p>
      {loadingOptions ? (
        <p className="text-sm text-muted-foreground">
          Checking the project's shared roles before publishing…
        </p>
      ) : blockedByMissingDestination ? (
        <p className="text-sm text-destructive" role="alert">
          This computer couldn't identify where the project's shared roles live.
          Publishing is blocked so existing project roles are not replaced.
        </p>
      ) : blockedByMissingImmutableBase ? (
        <p className="text-sm text-destructive" role="alert">
          The project's shared roles have no fixed base version. Refresh the
          project source before replacing its roles.
        </p>
      ) : options ? (
        <div className="space-y-2">
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
              {publication.status === "adopted" && !activation?.source ? (
                <p className="text-sm text-muted-foreground">
                  Resolving the adopted source on this computer…
                </p>
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
          {!activation.source ? (
            <p className="text-sm text-destructive" role="alert">
              The published roles could not be resolved on this computer. Reopen
              setup before installing roles.
            </p>
          ) : null}
          {activation.source &&
          activation.installation.status === "installed" ? (
            <ProjectTeamSetupRoster
              actions={{
                busy,
                creatingChannel,
                onRetryInstall: () => void install(),
                onStartLead: () => void startLead(),
                onRetryChannel: () => void createSessionChannel(),
              }}
              activation={activation}
              projectName={projectName}
              projectRef={draft.projectRef}
            />
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
        </section>
      ) : null}
      <ProjectTeamSetupFailureNotice failure={error} />
    </section>
  );
}
