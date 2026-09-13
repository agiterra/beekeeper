import * as React from "react";
import { useQueryClient } from "@tanstack/react-query";
import { refreshGlobalAgentConfig } from "@/features/agents/useGlobalAgentConfig";
import { buildCodingSessionTranscriptGenerationId } from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import { useAppNavigation } from "@/app/navigation/useAppNavigation";
import { useChannelsQuery } from "@/features/channels/hooks";
import { useProjectContainersQuery } from "@/features/projects-container/hooks";
import {
  projectSessionsChannelCandidates,
  resolveProjectSessionsChannel,
} from "@/features/projects-container/lib/projectSessionsChannel";
import { CodingSessionModelPicker } from "@/features/coding-sessions/ui/CodingSessionModelPicker";
import { CodingSessionTraitsPicker } from "@/features/coding-sessions/ui/CodingSessionTraitsPicker";
import {
  codingSessionModelChoices,
  joinCodingSessionModelId,
  splitCodingSessionModelId,
  resolveCodingSessionThinking,
  resolveCodingSessionContext,
} from "@/features/coding-sessions/lib/codingSessionModelChoice";
import { codingSessionCreateModelDisclosure } from "@/features/coding-sessions/lib/codingSessionCreateModel";
import {
  readCodingSessionModelFavorites,
  toggleCodingSessionModelFavorite,
  writeCodingSessionModelFavorites,
} from "@/features/coding-sessions/lib/codingSessionModelFavorites";
import { provisionCodingSessionProvider } from "@/shared/api/tauriSessionProvider";
import type { Channel } from "@/shared/api/types";
import { Button } from "@/shared/ui/button";
import {
  projectTeamSetupFailure,
  type ProjectTeamSetupFailure,
  type ProjectTeamSetupDraft,
  type ProjectTeamSetupAuthoringReservation,
  type ProjectTeamSetupLaunch,
} from "../lib/projectTeamSetup";
import {
  getProjectTeamSetupAuthoring,
  reserveProjectTeamSetupAuthoring,
  getProjectTeamSetupLaunch,
  startProjectTeamSetupAuthoring,
} from "../lib/projectTeamSetupApi";
import { useProjectTeamSetupRuntimes } from "../lib/useProjectTeamSetupRuntimes";
import type { ProjectTeamSetupLaunchObservation } from "./ProjectTeamSetupDraftView";
import { ProjectTeamSetupFailureNotice } from "./ProjectTeamSetupFailureNotice";

/** Receipt facts remain separate from a locally prepared or accepted request. */
export function projectTeamSetupLaunchSentence(
  launch: ProjectTeamSetupLaunch,
): string {
  switch (launch.status) {
    case "prepared":
      return "Authoring request prepared locally. It has not been confirmed by the relay.";
    case "awaiting_receipt":
      return "Authoring request accepted by the relay. Waiting for the provider's receipt.";
    case "ambiguous":
      return "Delivery is uncertain. Check status or retry the same saved request.";
    case "created":
      return "The provider confirmed creation of the authoring session.";
    case "initial_turn_failed":
      return "The authoring session was created, but its initial turn failed.";
    case "failed":
      return "The provider refused the authoring request.";
  }
}

/** Project channel discovery is read-only; the native start rechecks the binding. */
export function ProjectTeamSetupAuthoring({
  draft,
  onDraftMayChange,
  onLaunchObserved,
}: {
  draft: ProjectTeamSetupDraft;
  onDraftMayChange?: () => void;
  onLaunchObserved?: (launch: ProjectTeamSetupLaunchObservation) => void;
}) {
  const channels = useChannelsQuery({ includeSessionTransports: true });
  const projects = useProjectContainersQuery();
  const navigation = useAppNavigation();
  const queryClient = useQueryClient();
  const project = projects.data?.find(
    (entry) => entry.address === draft.projectRef,
  );
  const candidates = projectSessionsChannelCandidates(
    project ?? { address: draft.projectRef, channelIds: [] },
    channels.data ?? [],
  ).filter(
    (channel) =>
      !channel.archivedAt &&
      (!channel.projectRef || channel.projectRef === draft.projectRef),
  );
  const preferred = resolveProjectSessionsChannel({
    projectName: project?.name ?? "",
    projectChannels: candidates,
  });
  const channelError =
    channels.isError || projects.isError
      ? "Project channels could not be checked. Reopen setup after the connection recovers."
      : channels.data === undefined || projects.data === undefined
        ? "Checking this project's channels…"
        : null;
  return (
    <ProjectTeamSetupAuthoringControls
      key={`${draft.relayUrl}:${draft.ownerPubkey}:${draft.setupId}`}
      draft={draft}
      channels={candidates}
      preferredChannelId={preferred?.channelId ?? null}
      channelError={channelError}
      onDraftMayChange={onDraftMayChange}
      onLaunchObserved={onLaunchObserved}
      onProviderProvisioned={() => refreshGlobalAgentConfig(queryClient)}
      onOpen={(launch) => {
        if (launch.target)
          navigation.goCodingSession(
            launch.channelId,
            buildCodingSessionTranscriptGenerationId(
              launch.channelId,
              launch.providerPubkey,
              launch.target,
            ),
          );
      }}
    />
  );
}

/** Local authoring controls; mounting only reads discovery and durable records. */
export function ProjectTeamSetupAuthoringControls({
  draft,
  channels,
  preferredChannelId,
  channelError,
  onOpen,
  onDraftMayChange,
  onLaunchObserved,
  onProviderProvisioned,
}: {
  draft: ProjectTeamSetupDraft;
  channels: readonly Pick<Channel, "id" | "name">[];
  preferredChannelId: string | null;
  channelError: string | null;
  onOpen: (launch: ProjectTeamSetupLaunch) => void;
  onDraftMayChange?: () => void;
  /** Reports the saved launch after it is read, so the stage can follow it. */
  onLaunchObserved?: (launch: ProjectTeamSetupLaunchObservation) => void;
  onProviderProvisioned?: () => void;
}) {
  const scope = React.useMemo(
    () => ({
      projectRef: draft.projectRef,
      setupId: draft.setupId,
      expectedRelayUrl: draft.relayUrl,
    }),
    [draft.projectRef, draft.setupId, draft.relayUrl],
  );
  const runtimes = useProjectTeamSetupRuntimes(
    `${draft.relayUrl}:${draft.setupId}`,
  );
  const [reservation, setReservation] =
    React.useState<ProjectTeamSetupAuthoringReservation | null>(null);
  const [launch, setLaunch] = React.useState<ProjectTeamSetupLaunch | null>(
    null,
  );
  const [loaded, setLoaded] = React.useState(false);
  const [readFailed, setReadFailed] = React.useState(false);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<ProjectTeamSetupFailure | null>(
    null,
  );
  const [channelSelection, setChannelSelection] = React.useState<string | null>(
    null,
  );
  const [selection, setSelection] = React.useState<{
    instanceRef: string;
    model: string;
  } | null>(null);
  const [favorites, setFavorites] = React.useState<ReadonlySet<string>>(
    readCodingSessionModelFavorites,
  );
  React.useEffect(() => {
    let cancelled = false;
    void Promise.all([
      getProjectTeamSetupAuthoring(scope),
      getProjectTeamSetupLaunch(scope),
    ])
      .then(([saved, started]) => {
        if (cancelled) return;
        setReservation(saved);
        setLaunch(started);
        setLoaded(true);
        setReadFailed(false);
      })
      .catch((failure: unknown) => {
        if (cancelled) return;
        setReadFailed(true);
        setError(projectTeamSetupFailure(failure));
      });
    return () => {
      cancelled = true;
    };
  }, [scope]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: onLaunchObserved is a parent callback; report only when the observed record changes.
  React.useEffect(() => {
    if (loaded) onLaunchObserved?.(launch);
    else if (readFailed) onLaunchObserved?.("unreadable");
  }, [loaded, readFailed, launch]);
  const channelId =
    launch?.channelId ??
    reservation?.channelId ??
    channelSelection ??
    preferredChannelId ??
    (channels.length === 1 ? channels[0].id : "");
  const first = runtimes.data?.runtimes.find(
    (runtime) =>
      runtime.authState === "ready" &&
      runtime.capabilities.threadTurnStart &&
      runtimes.data?.models.has(runtime.instanceRef),
  );
  const instanceRef =
    launch?.providerInstanceRef ??
    selection?.instanceRef ??
    first?.instanceRef ??
    "";
  const runtime = runtimes.data?.runtimes.find(
    (entry) => entry.instanceRef === instanceRef,
  );
  const catalog = runtimes.data?.models.get(instanceRef);
  const model =
    launch?.model ??
    selection?.model ??
    (catalog?.allowedModels.includes(catalog.defaultModel)
      ? catalog.defaultModel
      : catalog?.allowedModels[0]) ??
    "";
  const selected = splitCodingSessionModelId(model);
  const choices = codingSessionModelChoices(catalog?.allowedModels ?? []);
  const locked = launch !== null;
  const ready =
    loaded &&
    !channelError &&
    channels.some((channel) => channel.id === channelId) &&
    runtime?.authState === "ready" &&
    runtime.capabilities.threadTurnStart &&
    catalog?.allowedModels.includes(model) &&
    !runtimes.loading;
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
  const start = async () => {
    // Refresh at the action boundary; a previously ready runtime may have signed out.
    const fresh = await runtimes.refresh();
    const current = fresh.runtimes.find(
      (entry) => entry.instanceRef === instanceRef,
    );
    if (
      current?.authState !== "ready" ||
      !current.capabilities.threadTurnStart ||
      (launch !== null && current.runtime !== launch.runtime) ||
      !fresh.models.get(instanceRef)?.allowedModels.includes(model)
    )
      throw new Error(
        "The selected local runtime or model is no longer available. Refresh and choose an available model.",
      );
    const provider = fresh.status.provisioned
      ? fresh.status
      : await provisionCodingSessionProvider(draft.relayUrl);
    if (!fresh.status.provisioned) onProviderProvisioned?.();
    if (!provider.providerPubkey)
      throw new Error("The local provider did not return its identity.");
    if (launch && provider.providerPubkey !== launch.providerPubkey)
      throw new Error(
        "The local provider identity changed. The saved authoring request was preserved.",
      );
    if (!reservation)
      setReservation(
        await reserveProjectTeamSetupAuthoring({ ...scope, channelId }),
      );
    try {
      onDraftMayChange?.();
      setLaunch(
        await startProjectTeamSetupAuthoring({
          ...scope,
          providerPubkey: provider.providerPubkey,
          providerInstanceRef: instanceRef,
          runtime: current.runtime,
          model,
        }),
      );
    } catch (failure) {
      // A failed response can still have saved an exact request. Recover its lock before another click.
      try {
        setLaunch(await getProjectTeamSetupLaunch(scope));
      } catch {
        setLoaded(false);
        setReadFailed(true);
      }
      throw failure;
    }
  };
  return (
    <section
      className="space-y-3 rounded-md border p-3"
      aria-label="Project roles authoring"
    >
      <h3 className="text-sm font-medium">Author project roles</h3>
      <p className="text-sm text-muted-foreground">
        Use an installed runtime on this computer to inspect the repository and
        adapt the draft. This does not publish role packs.
      </p>
      {channelError ? (
        <p className="text-sm">{channelError}</p>
      ) : channels.length === 0 ? (
        <p className="text-sm">
          This project has no available session channel. Create a project coding
          session first, then reopen setup.
        </p>
      ) : (
        <label className="block space-y-1 text-sm">
          Session channel
          <select
            aria-label="Authoring session channel"
            className="block w-full rounded-md border bg-background p-2 text-sm"
            disabled={busy || !!reservation || locked}
            value={channelId}
            onChange={(event) => setChannelSelection(event.target.value)}
          >
            <option value="">Choose a project channel</option>
            {channels.map((channel) => (
              <option key={channel.id} value={channel.id}>
                {channel.name}
              </option>
            ))}
          </select>
        </label>
      )}
      {locked ? (
        <p className="break-all text-sm">
          Saved request: {launch.runtime} · {launch.model}. Retries keep this
          runtime and model.
        </p>
      ) : (
        <>
          {runtime ? (
            <p className="text-sm">{runtime.label} · on this computer</p>
          ) : null}
          <CodingSessionModelPicker
            disabled={busy || runtimes.loading}
            favorites={favorites}
            selectionKey={instanceRef || null}
            model={selected.model || null}
            providers={(runtimes.data?.runtimes ?? []).map((entry) => ({
              selectionKey: entry.instanceRef,
              runtime: entry.runtime,
              label: entry.label,
              models: codingSessionModelChoices(
                runtimes.data?.models.get(entry.instanceRef)?.allowedModels ??
                  [],
              ).models,
              ready:
                entry.authState === "ready" &&
                entry.capabilities.threadTurnStart &&
                !!runtimes.data?.models.has(entry.instanceRef),
              unavailableNote:
                entry.authState === "missing"
                  ? "not installed"
                  : entry.authState === "needs_auth"
                    ? "sign-in needed"
                    : (runtimes.data?.errors.get(entry.instanceRef) ?? null),
            }))}
            onModelChange={(pick) => {
              const next = codingSessionModelChoices(
                runtimes.data?.models.get(pick.selectionKey)?.allowedModels ??
                  [],
              );
              setSelection({
                instanceRef: pick.selectionKey,
                model: joinCodingSessionModelId(
                  pick.model,
                  resolveCodingSessionThinking(
                    next,
                    pick.model,
                    selected.thinking,
                  ),
                  resolveCodingSessionContext(
                    next,
                    pick.model,
                    selected.context,
                  ),
                ),
              });
            }}
            onToggleFavorite={(key) =>
              setFavorites((previous) => {
                const next = toggleCodingSessionModelFavorite(previous, key);
                writeCodingSessionModelFavorites(next);
                return next;
              })
            }
          />
          <CodingSessionTraitsPicker
            disabled={busy || runtimes.loading}
            thinking={selected.thinking}
            context={selected.context}
            thinkingLevels={choices.thinkingByModel.get(selected.model) ?? []}
            contexts={choices.contextByModel.get(selected.model) ?? []}
            hasBareModel={choices.bareModels.has(selected.model)}
            onThinkingChange={(thinking) =>
              setSelection({
                instanceRef,
                model: joinCodingSessionModelId(
                  selected.model,
                  thinking,
                  selected.context,
                ),
              })
            }
            onContextChange={(context) =>
              setSelection({
                instanceRef,
                model: joinCodingSessionModelId(
                  selected.model,
                  selected.thinking,
                  context,
                ),
              })
            }
          />
        </>
      )}
      {codingSessionCreateModelDisclosure({ model, catalog }) ? (
        <p className="text-sm">
          {codingSessionCreateModelDisclosure({ model, catalog })}
        </p>
      ) : null}
      {runtimes.loading ? (
        <p className="text-sm">Checking installed runtimes and models…</p>
      ) : null}
      {!runtimes.loading && !first && !runtimes.error ? (
        <p className="text-sm">
          No authenticated local runtime has an available model catalog. Install
          or sign in through Settings → Harnesses, then refresh.
        </p>
      ) : null}
      {runtimes.error ? (
        <p role="alert" className="text-sm text-destructive">
          {runtimes.error}
        </p>
      ) : null}
      <div className="flex flex-wrap gap-2">
        <Button
          type="button"
          variant="outline"
          disabled={busy || runtimes.loading}
          onClick={() =>
            void run(async () => {
              await runtimes.refresh();
            })
          }
        >
          Refresh runtimes
        </Button>
        {!launch ||
        launch.status === "prepared" ||
        launch.status === "ambiguous" ||
        launch.status === "awaiting_receipt" ? (
          <Button
            type="button"
            className="max-w-full whitespace-normal"
            disabled={busy || !ready}
            onClick={() => void run(start)}
          >
            {busy
              ? "Working…"
              : launch
                ? "Retry saved authoring request"
                : "Start authoring session"}
          </Button>
        ) : null}
        <Button
          type="button"
          variant="outline"
          disabled={busy}
          onClick={() =>
            void run(async () => {
              const [saved, started] = await Promise.all([
                getProjectTeamSetupAuthoring(scope),
                getProjectTeamSetupLaunch(scope),
              ]);
              setReservation(saved);
              setLaunch(started);
              setLoaded(true);
              setReadFailed(false);
            })
          }
        >
          Check authoring status
        </Button>
        {launch?.target ? (
          <Button
            type="button"
            variant="outline"
            onClick={() => onOpen(launch)}
          >
            Open authoring session
          </Button>
        ) : null}
      </div>
      {reservation && !launch ? (
        <p className="text-sm">
          An authoring session is reserved locally. No execution has been
          confirmed.
        </p>
      ) : null}
      {launch ? (
        <div className="space-y-1 text-sm">
          <p role="status">{projectTeamSetupLaunchSentence(launch)}</p>
          {launch.message ? (
            <details className="text-muted-foreground">
              <summary className="cursor-pointer">Details</summary>
              <p className="break-words">{launch.message}</p>
            </details>
          ) : null}
        </div>
      ) : null}
      <ProjectTeamSetupFailureNotice failure={error} />
    </section>
  );
}
