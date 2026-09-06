import { useQuery } from "@tanstack/react-query";
import { CircleAlert, LoaderCircle, Rocket } from "lucide-react";
import * as React from "react";

import { useManagedAgentsQuery } from "@/features/agents/hooks";
import { useAppNavigation } from "@/app/navigation/useAppNavigation";
import { isSessionTransportChannel } from "@/shared/api/channelTypes";
import { useChannelsQuery } from "@/features/channels/hooks";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Textarea } from "@/shared/ui/textarea";
import { cn } from "@/shared/lib/cn";
import { MAX_CODING_SESSION_NAME_BYTES } from "@/features/coding-sessions/lib/codingSessionName";
import {
  NewCodingSessionChannelPicker,
  NewCodingSessionProjectDestination,
} from "./NewCodingSessionDestination";
import { useCodingSessionProviderCatalog } from "../useCodingSessionProviderCatalog";
import { useNewCodingSessionDraft } from "../lib/newCodingSessionDraft";
import { useNewCodingSessionPromptRecall } from "../lib/newCodingSessionPromptHistory";
import {
  codingSessionGoalOverflow,
  MAX_CODING_SESSION_GOAL_BYTES,
} from "../lib/codingSessionGoal";
import {
  codingSessionLaunchIsGoverned,
  codingSessionLaunchPlan,
  codingSessionLaunchReadiness,
  resolveCodingSessionLeadModel,
} from "../lib/codingSessionLaunchForm";
import {
  EMPTY_CODING_SESSION_POLICY_DRAFT,
  codingSessionPolicyDraftSetsAnything,
  type CodingSessionPolicyDraft,
} from "../lib/codingSessionPolicy";
import { codingSessionLeadWorktreeName } from "../lib/codingSessionWorktreeName";
import {
  isCodingSessionAuthFailure,
  isCodingSessionWorkdirFailure,
  isNewCodingSessionTargetReady,
  canRetryNewCodingSessionCreate,
  newCodingSessionStatusMessage,
  resolveNewCodingSessionTargets,
  resolveSelectedNewCodingSessionModel,
  resolveSelectedNewCodingSessionTarget,
  type NewCodingSessionTarget,
} from "../lib/newCodingSessionModel";
import {
  listCodingSessionCrewTeams,
  type CodingSessionCrewTeam,
} from "../lib/codingSessionCrewTeams";
import { teamReadinessLaunchGate } from "../lib/teamReadinessModel";
import { useProjectTeamReadiness } from "../lib/useProjectTeamReadiness";
import {
  NewCodingSessionModelDisclosure,
  NewCodingSessionProviderPicker,
  ProviderLoginNeeded,
  newCodingSessionEffectiveModel,
  newCodingSessionSeatModelBlocksCreate,
  resolveNewCodingSessionSeatModel,
} from "./NewCodingSessionProviderPicker";
import {
  NewCodingSessionBenchField,
  type NewCodingSessionBenchOption,
} from "./NewCodingSessionBenchField";
import {
  NewCodingSessionLeadField,
  resolveNewCodingSessionLead,
  type NewCodingSessionLeadCandidate,
} from "./NewCodingSessionLeadField";
import { NewCodingSessionPolicyField } from "./NewCodingSessionPolicyField";
import {
  formatNewCodingSessionSetupProvider,
  NewCodingSessionSetupDisclosure,
} from "./NewCodingSessionSetupDisclosure";
import { useNewCodingSessionTitleSuggestion } from "./useNewCodingSessionTitleSuggestion";
import { NewCodingSessionReadiness } from "./NewCodingSessionReadiness";
import { NewCodingSessionWorkdirField } from "./NewCodingSessionWorkdirField";
import { NewCodingSessionWorktreeField } from "./NewCodingSessionWorktreeField";
import { NewCodingSessionPendingView } from "./NewCodingSessionPendingView";
import { TeamReadinessCard } from "./TeamReadinessCard";
import {
  CodingSessionLaunchGoalNotes,
  CodingSessionLaunchSteps,
} from "./NewCodingSessionLaunchNotes";
import { useCodingSessionCrewLaunch } from "./useCodingSessionCrewLaunch";
import { useNewCodingSessionLaunchSubmit } from "./useNewCodingSessionLaunchSubmit";
import { useNewCodingSessionCreate } from "./useNewCodingSessionCreate";
import type { NewCodingSessionProjectContext } from "./NewCodingSessionDialog";

export const codingSessionCrewTeamsQueryKey = ["coding-session-crew-teams"];

/**
 * One form: goal · destination · saved setup · optional advanced settings.
 *
 * It replaced two tabs, and the two tabs are why it exists. *One session* and
 * *Team* were two answers to "what am I starting?", and each was missing what
 * the other had: the Team tab had no provider control at all, so a launch ran
 * the lead on whatever the One-session tab happened to be showing, and that
 * tab's model leaked into every unpinned seat (item 103, finding 12). Roles
 * were typed as free text. The lead's name was truncated to the point where
 * two Keystones read identically.
 *
 * Both defects are gone **by construction**, not by a check:
 *
 * - There is one model question, and it belongs to the lead. Nothing else is
 *   created by a launch, so there is no second seat for another control's
 *   value to reach. `resolveCodingSessionLeadModel` settles it from the
 *   identity or from an explicit pick, and from nothing else; neither, and the
 *   launch is blocked rather than borrowing one.
 * - Roles are read from the identity's own pack and rendered, never typed.
 * - The lead line carries the full name and the canonical short pubkey.
 *
 * Who leads decides the shape of everything below: an agent lead is a
 * governed session — genesis, authority chain, bench, policy — and leading it
 * yourself is one ungoverned execution, which is still a legitimate thing to
 * want and is what the single-create path has always done.
 */
export function NewCodingSessionForm({
  channelId: initialChannelId,
  onDone,
  projectContext = null,
}: {
  channelId?: string;
  /** Close the surface holding this form — a landed create, or a dismissal. */
  onDone: () => void;
  projectContext?: NewCodingSessionProjectContext | null;
}) {
  const { goCodingSession } = useAppNavigation();
  // Session transports are included on purpose: the provider-catalog
  // subscription and the project flow's channel summary need them. They are
  // filtered back out of the standalone picker below.
  const channelsQuery = useChannelsQuery({
    enabled: true,
    includeSessionTransports: true,
  });
  const memberChannels = React.useMemo(
    () =>
      (channelsQuery.data ?? [])
        .filter((channel) => channel.isMember)
        .sort((left, right) => left.name.localeCompare(right.name)),
    [channelsQuery.data],
  );
  const pickerChannels = React.useMemo(
    () =>
      memberChannels.filter((channel) => !isSessionTransportChannel(channel)),
    [memberChannels],
  );
  const [channelSelection, setChannelSelection] = React.useState<string | null>(
    initialChannelId ?? null,
  );
  const channelId = projectContext
    ? projectContext.channelId
    : (channelSelection ??
      (pickerChannels.some((channel) => channel.id === initialChannelId)
        ? (initialChannelId ?? null)
        : (pickerChannels[0]?.id ?? null)));
  const memberChannelIds = React.useMemo(
    () => memberChannels.map((channel) => channel.id).sort(),
    [memberChannels],
  );
  const providerCatalog = useCodingSessionProviderCatalog(memberChannelIds);

  const pendingChannelKey = projectContext
    ? `pending-project-channel:${projectContext.projectId}`
    : null;
  const targetChannelId = channelId ?? pendingChannelKey;
  const scopeId = projectContext
    ? `project:${projectContext.projectId}`
    : (channelId ?? "unscoped");

  const {
    beginLoginWatch,
    durabilityError,
    hostPhase,
    isPublishing,
    lifecycle,
    lifecycleErrorMessage,
    lifecycleIsLoading,
    providerStatus,
    providerRuntimes,
    providerModelsByInstanceRef,
    publishError,
    retryExact,
    refreshProviderState,
    seat: signedSeat,
    seatPackRef,
    seatPackStaged,
    stalled,
    startFresh,
    submit,
    transaction,
  } = useNewCodingSessionCreate({
    scopeId,
    onCreated: ({ channelId: createdChannelId, generationId }) => {
      onDone();
      void goCodingSession(createdChannelId, generationId, { replace: true });
    },
  });

  const targets = React.useMemo<NewCodingSessionTarget[]>(
    () =>
      resolveNewCodingSessionTargets({
        catalogs: providerCatalog.entries,
        channelId: targetChannelId,
        localProvider: providerStatus?.providerPubkey
          ? {
              providerPubkey: providerStatus.providerPubkey,
              runtimes: providerRuntimes,
              modelsByInstanceRef: providerModelsByInstanceRef,
            }
          : null,
      }),
    [
      providerCatalog.entries,
      providerModelsByInstanceRef,
      providerRuntimes,
      providerStatus?.providerPubkey,
      targetChannelId,
    ],
  );

  const [targetSelection, setTargetSelection] = React.useState<{
    key: string | null;
    explicit: boolean;
  }>({ key: null, explicit: false });
  const selectedTarget = resolveSelectedNewCodingSessionTarget({
    targets,
    selectedTargetKey: targetSelection.key,
    selectionExplicit: targetSelection.explicit,
  });
  const refreshRuntimeTarget = React.useCallback(async () => {
    const refreshed = await refreshProviderState();
    return resolveSelectedNewCodingSessionTarget({
      targets: resolveNewCodingSessionTargets({
        catalogs: providerCatalog.entries,
        channelId: targetChannelId,
        localProvider: refreshed.status.providerPubkey
          ? {
              providerPubkey: refreshed.status.providerPubkey,
              runtimes: refreshed.runtimes,
              modelsByInstanceRef: refreshed.modelsByInstanceRef,
            }
          : null,
      }),
      selectedTargetKey: targetSelection.key,
      selectionExplicit: targetSelection.explicit,
    });
  }, [
    providerCatalog.entries,
    refreshProviderState,
    targetSelection.explicit,
    targetSelection.key,
    targetChannelId,
  ]);
  const [modelSelection, setModelSelection] = React.useState<{
    value: string | null;
    explicit: boolean;
  }>({ value: null, explicit: false });
  const providerModel = resolveSelectedNewCodingSessionModel({
    provider: selectedTarget?.provider ?? null,
    selectedModel: modelSelection.value,
    selectionExplicit: modelSelection.explicit,
  });

  const managedAgentsQuery = useManagedAgentsQuery();
  const managedAgents = React.useMemo(
    () => managedAgentsQuery.data ?? [],
    [managedAgentsQuery.data],
  );
  // Read only to learn which role each identity carries: a role assignment
  // record names what an identity is *for* in this project, and the launch
  // never creates more than the lead's own seat.
  const roleHintsQuery = useQuery({
    queryKey: codingSessionCrewTeamsQueryKey,
    queryFn: listCodingSessionCrewTeams,
    staleTime: 30_000,
  });
  const roleHintRecords = React.useMemo<CodingSessionCrewTeam[]>(
    () => roleHintsQuery.data ?? [],
    [roleHintsQuery.data],
  );
  const candidates = React.useMemo<NewCodingSessionLeadCandidate[]>(
    () =>
      managedAgents.map((agent) => ({
        pubkey: agent.pubkey,
        name: agent.name,
        // Read, never typed. A role record wins over the home role because it
        // names what the identity is *for* in this project.
        role:
          roleHintRecords
            .flatMap((team) => team.crew.seats)
            .find((seat) => seat.personaId === agent.personaId)?.role ??
          agent.homeRole ??
          null,
        model: agent.model,
        ...(agent.hasRolePack === undefined
          ? {}
          : { hasRolePack: agent.hasRolePack }),
      })),
    [roleHintRecords, managedAgents],
  );

  const [leadActor, setLeadActor] = React.useState<string | null>(
    projectContext?.defaultSeat?.actor ?? null,
  );
  const lead = React.useMemo(
    () =>
      resolveNewCodingSessionLead({
        actor: leadActor,
        candidates,
        youLabel: "You",
      }),
    [candidates, leadActor],
  );
  const governed = codingSessionLaunchIsGoverned(lead);

  const modelCatalog = selectedTarget
    ? (providerModelsByInstanceRef.get(
        selectedTarget.provider.providerInstanceRef,
      ) ?? {
        defaultModel: selectedTarget.provider.defaultModel,
        allowedModels: selectedTarget.provider.allowedModels,
      })
    : null;
  const seatedModel = resolveNewCodingSessionSeatModel({
    agentModel: lead.kind === "agent" ? lead.model : null,
    allowedModels: modelCatalog?.allowedModels ?? [],
    providerInstanceRef:
      selectedTarget?.provider.providerInstanceRef ?? "this runtime",
    selectionExplicit: modelSelection.explicit,
  });
  // What the picker *shows*. Kept for the picker and its disclosure, which are
  // about the runtime's catalog, and deliberately NOT what gets published.
  const effectiveModel = newCodingSessionEffectiveModel({
    seatModel: seatedModel,
    providerModel,
  });
  // What gets published. Two sources — the identity, or an explicit pick — and
  // never the picker's default for an agent that declared none (REVIEW-B3 F1).
  // An override is a person choosing weights over the identity's own, and it
  // needs a reason for the same reason `bee sessions hire --override-model`
  // does: a model nobody can account for is a seat running weights nobody
  // chose.
  const { model: leadModel, overridden: modelOverridden } =
    resolveCodingSessionLeadModel({
      lead,
      pickedModel: modelSelection.value,
      pickedExplicitly: modelSelection.explicit,
      providerModel: effectiveModel,
    });
  const [overrideReason, setOverrideReason] = React.useState("");

  // One goal, whoever leads. Two states — one for the governed launch and one
  // for the create — is exactly the two-tab defect in miniature: switching the
  // lead would silently empty the field the person had already filled in, and
  // the button would go dead with "Write the goal" under a goal they wrote.
  const {
    text: goal,
    setText: setGoal,
    clear: clearDraft,
    persistence: draftPersistence,
  } = useNewCodingSessionDraft(scopeId);
  // ⌘↑/⌘↓ walks back through the goals already launched from this dialog —
  // the session composer's shortcut and stepping, so the two recall surfaces
  // cannot drift apart under the same keystroke.
  const promptRecall = useNewCodingSessionPromptRecall({
    scopeId,
    text: goal,
    setText: setGoal,
  });

  const [title, setTitle] = React.useState("");
  const naming = useNewCodingSessionTitleSuggestion({
    firstMessage: goal,
    title,
    setTitle,
  });

  const [benchIdentities, setBenchIdentities] = React.useState<string[]>([]);
  const [benchProviders, setBenchProviders] = React.useState<string[]>([]);
  const [challengerRate, setChallengerRate] = React.useState<number | null>(
    null,
  );
  const [policyDraft, setPolicyDraft] =
    React.useState<CodingSessionPolicyDraft>(EMPTY_CODING_SESSION_POLICY_DRAFT);
  // The bench IS policy: `bench.identities` and `bench.providers` are 44245
  // fields, so the one record carries both halves and there is nothing for a
  // reader to reconcile.
  const policy = React.useMemo<CodingSessionPolicyDraft>(
    () => ({
      ...policyDraft,
      benchIdentities,
      benchProviders,
      challengerSampleRate: challengerRate,
    }),
    [benchIdentities, benchProviders, challengerRate, policyDraft],
  );
  const policySet = codingSessionPolicyDraftSetsAnything(policy);

  const [workdir, setWorkdir] = React.useState("");
  const [useWorktree, setUseWorktree] = React.useState(true);
  const [worktreeName, setWorktreeName] = React.useState("");
  const [worktreeSource, setWorktreeSource] = React.useState<string | null>(
    null,
  );

  /**
   * The roles this launch actually seats: the lead's, plus every benched
   * identity's own role.
   *
   * REVIEW-L2 F7: both call sites passed the lead's role alone, so Prepare
   * confirmed **one** name for a two-seat launch and refreshed the builder pack
   * the bench seat will be hired against silently — counted in `N other packs
   * were refreshed…`. `candidates` already carries each identity's role, which
   * is what `benchIdentityOptions` renders as its detail line.
   */
  const launchRoles = React.useMemo(() => {
    const roles = new Set<string>();
    if (lead.kind === "agent") roles.add(lead.role);
    for (const pubkey of benchIdentities) {
      const role = candidates.find(
        (candidate) => candidate.pubkey === pubkey,
      )?.role;
      if (role) roles.add(role);
    }
    return [...roles].sort();
  }, [benchIdentities, candidates, lead]);
  const teamReadiness = useProjectTeamReadiness({
    projectRef: projectContext?.projectRef ?? null,
    checkoutPath: projectContext?.defaultWorkdir ?? null,
    selectedRoles: launchRoles,
    channelIds: channelId === null ? [] : [channelId],
    refreshRuntimeTargets: async () => {
      const refreshed = await refreshRuntimeTarget();
      if (!refreshed || !isNewCodingSessionTargetReady(refreshed)) {
        throw new Error(
          refreshed?.availability?.hint ??
            "Provider setup completed, but no installed and authenticated runtime target is ready.",
        );
      }
    },
  });
  const readinessGate = teamReadinessLaunchGate({
    projectRef: projectContext?.projectRef ?? null,
    loading: teamReadiness.isLoading,
    error: teamReadiness.readError,
    readiness: teamReadiness.readiness,
    runtimeTarget: selectedTarget,
  });

  const { isLaunching, launch, result, steps } = useCodingSessionCrewLaunch({
    ensureChannelId: projectContext?.ensureChannelId ?? null,
    workdir: workdir.trim().length > 0 ? workdir.trim() : null,
    title: title.trim().length > 0 ? title.trim() : null,
    policy: policySet ? policy : null,
  });
  const [launchError, setLaunchError] = React.useState<string | null>(null);
  const [setupError, setSetupError] = React.useState<string | null>(null);
  const [isPreparingChannel, setIsPreparingChannel] = React.useState(false);
  // Every busy state, as the sentence a person reads. It reaches the button
  // through readiness rather than beside it, so a disabled control can never
  // be silent — during channel preparation and the readiness preflight it used
  // to be (REVIEW-B3 F7).
  const busySentence = isPublishing
    ? "Publishing this session…"
    : isLaunching
      ? "Launching…"
      : isPreparingChannel
        ? "Preparing this project's sessions channel…"
        : transaction !== null
          ? "A session request from this dialog is already in flight."
          : teamReadiness.isLaunchPreflighting
            ? "Re-checking readiness before launching…"
            : teamReadiness.isPreparing
              ? "Preparing the project's roles…"
              : teamReadiness.isScanning
                ? "Scanning the project's role packs…"
                : null;
  const interactionLocked = busySentence !== null;

  const goalOverflow = React.useMemo(
    () => codingSessionGoalOverflow(goal),
    [goal],
  );
  const goalBytes = React.useMemo(
    () => new TextEncoder().encode(goal.trim()).byteLength,
    [goal],
  );
  const unresolvedBenchIdentities = React.useMemo(
    () =>
      benchIdentities.filter(
        (pubkey) => !candidates.some((entry) => entry.pubkey === pubkey),
      ),
    [benchIdentities, candidates],
  );
  const readiness = codingSessionLaunchReadiness({
    channelId,
    canCreateChannel: projectContext?.ensureChannelId !== undefined,
    goal,
    goalOverflow,
    lead,
    governed,
    providerInstanceRef: selectedTarget?.provider.providerInstanceRef ?? null,
    providerAuthorityPubkey: selectedTarget?.signerPubkey ?? null,
    leadModel,
    modelOverrideReason: overrideReason,
    modelOverridden,
    providerRefusal: newCodingSessionSeatModelBlocksCreate(seatedModel)
      ? seatedModel.note
      : null,
    projectReadiness: projectContext?.projectRef
      ? { allowed: readinessGate.allowed, reason: readinessGate.reason }
      : null,
    projectReadinessUnknown:
      projectContext?.projectRef !== undefined &&
      projectContext?.projectRef !== null &&
      (teamReadiness.isLoading || teamReadiness.readError !== null),
    ...(lead.kind === "agent" && lead.hasRolePack !== undefined
      ? { leadHasRolePack: lead.hasRolePack }
      : {}),
    policySet,
    unresolvedBenchIdentities,
    busySentence,
  });
  const configurationBlocker = readiness.blockers.some(
    (blocker) =>
      (blocker.id === "provider" &&
        providerStatus !== null &&
        selectedTarget === null) ||
      blocker.id === "provider-refusal" ||
      blocker.id === "model" ||
      blocker.id === "override-reason" ||
      blocker.id === "project-readiness" ||
      blocker.id.startsWith("bench:"),
  );
  const [configurationOpen, setConfigurationOpen] = React.useState(false);
  React.useEffect(() => {
    if (configurationBlocker) setConfigurationOpen(true);
  }, [configurationBlocker]);
  const plan = codingSessionLaunchPlan({
    governed,
    lead,
    goal,
    policySet,
    benchCount: benchIdentities.length,
  });
  // One expression, and every disabled state has its sentence: `busySentence`
  // is a blocker like any other, so this is exactly `blockers.length === 0`.
  const canLaunch = readiness.canLaunch;

  const status = newCodingSessionStatusMessage({
    hostPhase,
    isPublishing,
    publishError: publishError ?? durabilityError,
    lifecycle,
    authRuntime: null,
    stalled,
    publishState: transaction?.publishState ?? null,
  });
  const failureCode =
    lifecycle?.state === "failed" ? lifecycle.error.code : undefined;

  const handleLaunch = useNewCodingSessionLaunchSubmit({
    canLaunch,
    candidates,
    channelId,
    clearDraft,
    leadModel,
    goCodingSession,
    goal,
    governed,
    launch,
    lead,
    onDone,
    policySet,
    projectContext,
    refreshRuntimeTarget,
    rememberPrompt: promptRecall.remember,
    selectedTarget,
    setIsPreparingChannel,
    setLaunchError,
    setSetupError,
    submit,
    title,
    useWorktree,
    workdir,
    worktreeName,
    worktreeSource,
  });

  const [editRequested, setEditRequested] = React.useState(false);
  React.useEffect(() => {
    if (transaction === null) setEditRequested(false);
  }, [transaction]);
  const handleStartFresh = React.useCallback(() => {
    setEditRequested(false);
    startFresh();
  }, [startFresh]);

  if (transaction !== null && !editRequested) {
    return (
      <NewCodingSessionPendingView
        beginLoginWatch={beginLoginWatch}
        channelName={
          projectContext
            ? null
            : (memberChannels.find(
                (channel) => channel.id === transaction.input.channelId,
              )?.name ?? null)
        }
        durabilityError={durabilityError}
        hostPhase={hostPhase}
        isPublishing={isPublishing}
        lifecycle={lifecycle}
        lifecycleErrorMessage={lifecycleErrorMessage}
        lifecycleIsLoading={lifecycleIsLoading}
        onClose={onDone}
        onEditRequest={() => setEditRequested(true)}
        projectName={projectContext?.projectName ?? null}
        publishError={publishError}
        retryExact={retryExact}
        seatLabel={
          signedSeat
            ? (managedAgents.find((agent) => agent.pubkey === signedSeat.actor)
                ?.name ?? null)
            : null
        }
        seatPackRef={seatPackRef}
        seatPackStaged={seatPackStaged}
        signedSeat={signedSeat}
        stalled={stalled}
        startFresh={handleStartFresh}
        transaction={transaction}
      />
    );
  }

  const benchIdentityOptions: NewCodingSessionBenchOption[] = candidates
    .filter(
      (candidate) =>
        candidate.role !== null &&
        !(lead.kind === "agent" && candidate.pubkey === lead.actor),
    )
    .map((candidate) => ({
      value: candidate.pubkey,
      label: candidate.name,
      detail: candidate.role,
    }));
  const benchProviderOptions: NewCodingSessionBenchOption[] = targets.map(
    (target) => ({
      value: target.provider.providerInstanceRef,
      label: target.availability?.label ?? target.provider.runtime,
      detail: null,
    }),
  );

  return (
    <>
      <div
        // `overflow-y-auto` clips the x-axis as well, so a `w-full` child sits
        // exactly on the clip edge and loses its side borders; one pixel of
        // horizontal padding gives them somewhere to land.
        className="-mx-px flex max-h-[65vh] min-h-0 flex-col gap-5 overflow-y-auto px-px"
        data-testid="new-coding-session-form"
      >
        <div className="flex flex-col gap-2">
          <label
            className="text-xs font-medium text-muted-foreground"
            htmlFor="coding-session-goal"
          >
            Goal
          </label>
          <Textarea
            className="min-h-32"
            data-testid="new-coding-session-goal"
            disabled={interactionLocked}
            id="coding-session-goal"
            maxLength={MAX_CODING_SESSION_GOAL_BYTES}
            onBlur={naming.requestNow}
            onChange={(event) => setGoal(event.target.value)}
            onKeyDown={promptRecall.onKeyDown}
            placeholder="What is this session for?"
            value={goal}
          />
          <CodingSessionLaunchGoalNotes
            bytes={goalBytes}
            goalOutcome={result?.goal ?? null}
            overflow={goalOverflow}
          />
          {draftPersistence.message ? (
            <p className="text-2xs text-muted-foreground">
              {draftPersistence.message}
            </p>
          ) : null}
        </div>

        {projectContext ? (
          <NewCodingSessionProjectDestination
            projectName={projectContext.projectName}
          />
        ) : (
          <NewCodingSessionChannelPicker
            channels={pickerChannels}
            disabled={interactionLocked}
            onChange={(next) => {
              setChannelSelection(next);
              setTargetSelection({ key: null, explicit: false });
              setModelSelection({ value: null, explicit: false });
            }}
            value={channelId}
          />
        )}

        <NewCodingSessionSetupDisclosure
          configurationOpen={configurationOpen}
          governed={governed}
          onConfigurationOpenChange={setConfigurationOpen}
          setupLead={
            lead.kind === "agent" ? `${lead.label} · ${lead.role}` : lead.label
          }
          setupModel={
            leadModel ?? effectiveModel ?? "Model will be chosen at runtime"
          }
          setupProvider={formatNewCodingSessionSetupProvider(selectedTarget)}
        >
          <NewCodingSessionLeadField
            candidates={candidates}
            disabled={interactionLocked}
            lead={lead}
            onLeadChange={setLeadActor}
          />

          <div className="flex flex-col gap-2">
            <NewCodingSessionProviderPicker
              disabled={interactionLocked}
              model={effectiveModel}
              onLoginLaunched={({ runtime }) => beginLoginWatch(runtime)}
              onModelChange={(value) =>
                setModelSelection({ value, explicit: true })
              }
              onTargetChange={(key) => {
                setTargetSelection({ key, explicit: true });
                setModelSelection({ value: null, explicit: false });
              }}
              selectedTarget={selectedTarget}
              targets={targets}
            />
            <NewCodingSessionModelDisclosure
              catalog={modelCatalog}
              model={effectiveModel}
              note={seatedModel.note}
            />
            {modelOverridden ? (
              <div className="flex flex-col gap-1">
                <label
                  className="text-2xs text-muted-foreground"
                  htmlFor="coding-session-model-override"
                >
                  Why this model, and not the one {lead.label} carries?
                </label>
                <Input
                  data-testid="new-coding-session-model-override"
                  disabled={interactionLocked}
                  id="coding-session-model-override"
                  onChange={(event) => setOverrideReason(event.target.value)}
                  placeholder="Because…"
                  value={overrideReason}
                />
              </div>
            ) : null}
          </div>

          {governed ? (
            <NewCodingSessionBenchField
              challengerRate={challengerRate}
              disabled={interactionLocked}
              identities={benchIdentityOptions}
              onChallengerRateChange={setChallengerRate}
              onToggleIdentity={(value, selected) =>
                setBenchIdentities((previous) =>
                  selected
                    ? [...new Set([...previous, value])]
                    : previous.filter((entry) => entry !== value),
                )
              }
              onToggleProvider={(value, selected) =>
                setBenchProviders((previous) =>
                  selected
                    ? [...new Set([...previous, value])]
                    : previous.filter((entry) => entry !== value),
                )
              }
              providers={benchProviderOptions}
              selectedIdentities={benchIdentities}
              selectedProviders={benchProviders}
            />
          ) : null}

          {governed ? (
            <NewCodingSessionPolicyField
              disabled={interactionLocked}
              draft={policyDraft}
              onDraftChange={setPolicyDraft}
            />
          ) : null}

          <div className="flex flex-col gap-2">
            <label
              className="text-xs font-medium text-muted-foreground"
              htmlFor="coding-session-title"
            >
              Name <span className="font-normal">(optional)</span>
            </label>
            <Input
              data-testid="new-coding-session-title"
              disabled={interactionLocked}
              id="coding-session-title"
              maxLength={MAX_CODING_SESSION_NAME_BYTES}
              onChange={(event) => naming.setTitleByHand(event.target.value)}
              placeholder="What is this session for?"
              value={title}
            />
            {naming.status.message ? (
              <p
                className={cn(
                  "text-2xs",
                  naming.status.state === "failed"
                    ? "text-destructive"
                    : "text-muted-foreground",
                )}
                data-testid="new-coding-session-title-suggestion"
              >
                {naming.status.message}
              </p>
            ) : null}
          </div>

          <NewCodingSessionWorktreeField
            checked={useWorktree}
            disabled={
              interactionLocked && !isCodingSessionWorkdirFailure(failureCode)
            }
            name={worktreeName}
            onCheckedChange={setUseWorktree}
            onNameChange={setWorktreeName}
            onSourceChange={setWorktreeSource}
            sessionName={
              governed
                ? codingSessionLeadWorktreeName(title.trim() || "session")
                : title
            }
            source={worktreeSource}
            workdir={workdir}
          />

          <NewCodingSessionWorkdirField
            channelId={channelId}
            disabled={
              interactionLocked && !isCodingSessionWorkdirFailure(failureCode)
            }
            fallbackPath={projectContext?.defaultWorkdir ?? null}
            onChange={setWorkdir}
            projectKey={projectContext?.projectRef ?? null}
            usesWorktree={useWorktree}
            value={workdir}
          />

          {projectContext?.projectRef ? (
            <TeamReadinessCard
              loading={teamReadiness.isLoading}
              externalBusy={interactionLocked}
              names={teamReadiness.names}
              onBeginPrepare={() => void teamReadiness.beginPrepare()}
              onCancelPrepare={teamReadiness.cancelPrepare}
              onConfirmPrepare={() => void teamReadiness.confirmPrepare()}
              onNameChange={teamReadiness.setName}
              prepareError={teamReadiness.prepareError}
              prepareWarning={teamReadiness.prepareWarning}
              prepareSteps={teamReadiness.prepareSteps}
              preparing={teamReadiness.isPreparing}
              readError={teamReadiness.readError}
              readiness={teamReadiness.readiness}
              scan={teamReadiness.scan}
              scanning={teamReadiness.isScanning}
              selectedRoles={launchRoles}
              runtimeTarget={selectedTarget}
            />
          ) : null}
        </NewCodingSessionSetupDisclosure>

        {steps.length > 0 ? <CodingSessionLaunchSteps steps={steps} /> : null}

        <NewCodingSessionReadiness plan={plan} readiness={readiness} />

        {isCodingSessionAuthFailure(failureCode) ? (
          <ProviderLoginNeeded
            onLoginLaunched={({ runtime }) => beginLoginWatch(runtime)}
            runtime={
              selectedTarget
                ? {
                    runtime: selectedTarget.provider.runtime,
                    label: selectedTarget.availability?.label,
                  }
                : null
            }
          />
        ) : null}

        {status ? (
          <p
            className={cn(
              "flex items-start gap-2 text-sm",
              status.tone === "destructive"
                ? "text-destructive"
                : "text-muted-foreground",
            )}
            data-testid="new-coding-session-status"
            role="status"
          >
            {status.tone === "destructive" ? (
              <CircleAlert className="mt-0.5 size-4 shrink-0" />
            ) : (
              <LoaderCircle className="mt-0.5 size-4 shrink-0 animate-spin motion-reduce:animate-none" />
            )}
            {status.message}
          </p>
        ) : null}
      </div>

      {/* Outside the scroll box: a launch button that scrolls away with the
          fields is a launch button a short window hides entirely. */}
      <div className="flex shrink-0 flex-wrap items-center justify-end gap-2">
        {[setupError, launchError].map((message) =>
          message ? (
            <p
              className="flex max-h-32 basis-full items-start gap-2 overflow-y-auto break-words text-sm text-destructive"
              data-testid="new-coding-session-setup-error"
              key={message}
              role="alert"
            >
              <CircleAlert className="mt-0.5 size-4 shrink-0" />
              {message}
            </p>
          ) : null,
        )}
        {transaction ? (
          <>
            <Button
              data-testid="new-coding-session-start-fresh"
              onClick={handleStartFresh}
              type="button"
              variant={stalled ? "outline" : "ghost"}
            >
              Start fresh
            </Button>
            <Button
              data-testid="new-coding-session-retry"
              disabled={
                !canRetryNewCodingSessionCreate({
                  isPublishing,
                  lifecycleIsLoading,
                  lifecycleErrorMessage,
                  lifecycleState: lifecycle?.state ?? null,
                  stalled,
                  publishState: transaction?.publishState ?? null,
                })
              }
              onClick={retryExact}
              type="button"
              variant="outline"
            >
              Retry this exact request
            </Button>
          </>
        ) : null}
        <Button
          data-testid="new-coding-session-submit"
          disabled={!canLaunch}
          onClick={handleLaunch}
          type="button"
        >
          {isLaunching || isPublishing ? (
            <LoaderCircle className="animate-spin motion-reduce:animate-none" />
          ) : (
            <Rocket />
          )}
          Start
        </Button>
      </div>
    </>
  );
}
