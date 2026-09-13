import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { useManagedAgentsQuery } from "@/features/agents/hooks";
import { useCommunities } from "@/features/communities/useCommunities";
import { useCodingSessionProject } from "@/features/projects-container/hooks";
import {
  installedRolesForProject,
  useProjectInstalledRolesQuery,
} from "@/features/roles/lib/projectInstalledRoles";
import {
  listCodingSessionCrewTeams,
  type CodingSessionCrewTeam,
} from "../../lib/codingSessionCrewTeams";
import {
  applyCodingSessionInstalledRoles,
  codingSessionInstalledLeadDefault,
  groupCodingSessionCandidates,
  resolveCodingSessionLeadActor,
} from "../../lib/codingSessionLeadCandidateGroups";
import {
  type CodingSessionFoundedDraft,
  readCodingSessionFoundedDraft,
  writeCodingSessionFoundedDraft,
} from "../../lib/codingSessionFoundedDraft";
import {
  type CodingSessionLaunchLead,
  resolveCodingSessionLeadModel,
} from "../../lib/codingSessionLaunchForm";
import {
  EMPTY_CODING_SESSION_POLICY_DRAFT,
  codingSessionPolicyDraftSetsAnything,
  type CodingSessionPolicyDraft,
} from "../../lib/codingSessionPolicy";
import {
  type CodingSessionSetupMode,
  readCodingSessionSetupMode,
  writeCodingSessionSetupMode,
} from "../../lib/codingSessionSetupMode";
import type { NewCodingSessionWorkspaceReuse } from "../../lib/codingSessionWorkspaceReuse";
import {
  isNewCodingSessionTargetReady,
  newCodingSessionStatusMessage,
  resolveNewCodingSessionTargets,
  resolveSelectedNewCodingSessionModel,
  resolveSelectedNewCodingSessionTarget,
  type NewCodingSessionTarget,
} from "../../lib/newCodingSessionModel";
import { teamReadinessLaunchGate } from "../../lib/teamReadinessModel";
import { useProjectTeamReadiness } from "../../lib/useProjectTeamReadiness";
import { useCodingSessionProviderCatalog } from "../../useCodingSessionProviderCatalog";
import type { NewCodingSessionBenchOption } from "../NewCodingSessionBenchField";
import {
  resolveNewCodingSessionLead,
  type NewCodingSessionLeadCandidate,
} from "../NewCodingSessionLeadField";
import {
  newCodingSessionEffectiveModel,
  newCodingSessionSeatModelBlocksCreate,
  resolveNewCodingSessionSeatModel,
} from "../NewCodingSessionProviderPicker";
import { useCodingSessionCrewLaunch } from "../useCodingSessionCrewLaunch";
import { useNewCodingSessionCreate } from "../useNewCodingSessionCreate";
import type { CodingSessionFoundedGoal } from "./CodingSessionFoundedWorkspace";
import {
  codingSessionFoundedBusySentence,
  codingSessionFoundedReadiness,
} from "./codingSessionFoundedReadiness";
import {
  type CodingSessionFoundedTextDeps,
  useCodingSessionFoundedText,
} from "./useCodingSessionFoundedText";

/** Role-assignment records, read once per screen to label each identity. */
export const codingSessionCrewTeamsQueryKey = ["coding-session-crew-teams"];

/** Solo's lead: the founder, always, whatever the Team picker last held. */
const YOU_LEAD: CodingSessionLaunchLead = { kind: "you", label: "You" };

/** A founded draft with nothing recorded — what a page founded elsewhere reads. */
const EMPTY_FOUNDED_DRAFT: CodingSessionFoundedDraft = {
  name: null,
  workdir: null,
  useWorktree: true,
  worktreeName: null,
  worktreeSource: null,
  rememberWorkspace: true,
  repoRef: null,
  workspaceSourcePath: null,
  workspaceSourceBranch: null,
  workspaceSourceBranchSource: null,
};

/**
 * The setup card's state: everything the launch dialog used to hold, on the
 * founded page — Solo or Team, the name and the initial prompt, who leads,
 * runtime and model (with the override reason), bench and challenger rate,
 * policy, roles readiness, and where it runs — because every session is now
 * founded on the click and set up here (Andy, 2026-09-10).
 *
 * Provider status, runtimes and models come from the same create hook the
 * dialog mounted, scoped to this umbrella, so a Solo create and a Team crew
 * launch read one runtime picture. The name and the prompt are the text
 * hook's: they publish on commit under the rules written there, and Start
 * flushes them before it creates anything.
 */
export function useCodingSessionFoundedSetup(input: {
  channelId: string;
  sessionRef: string;
  projectRef: string | null;
  /** The genesis signer — the identity whose 44229 and 44227 are this session's. */
  founderPubkey: string;
  /** The umbrella's goal as the page could read it. */
  goal: CodingSessionFoundedGoal;
  /** The founder-keyed 44229, or null when none is on the wire. */
  wireName: string | null;
  /** Whether the names read has settled once for this channel. */
  nameResolved: boolean;
  /** Failed or incomplete name history must not enable a name write. */
  nameReadError?: string | null;
  /** Explicitly retry name history and a failed live watch. */
  refreshNames?: () => void;
  /**
   * Whether the channel record — and so `projectRef` — has been read. Until
   * it has, a null `projectRef` means "not read", not "no project", and Start
   * is held rather than signing that guess.
   */
  channelReader: "loading" | "errored" | "resolved";
  /** True while a receipt-joined create claims the umbrella (see busy sentence). */
  starting?: boolean;
  onCreated: (input: { channelId: string; generationId: string }) => void;
  /** Injected in tests; the relay by default. */
  textDeps?: CodingSessionFoundedTextDeps;
}) {
  const {
    channelId,
    sessionRef,
    projectRef,
    goal,
    wireName,
    nameResolved,
    nameReadError = null,
    refreshNames,
    channelReader,
    onCreated,
    textDeps,
  } = input;
  const starting = input.starting === true;
  const channelIds = React.useMemo(() => [channelId], [channelId]);
  const providerCatalog = useCodingSessionProviderCatalog(channelIds);
  const create = useNewCodingSessionCreate({
    scopeId: `founded:${sessionRef}`,
    onCreated,
  });
  const {
    providerModelsByInstanceRef,
    providerRuntimes,
    providerStatus,
    refreshProviderState,
  } = create;

  // The mode this computer used last; written on every change.
  const [mode, setModeState] = React.useState<CodingSessionSetupMode>(() =>
    readCodingSessionSetupMode(),
  );
  const setMode = React.useCallback((next: CodingSessionSetupMode) => {
    setModeState(next);
    writeCodingSessionSetupMode(next);
  }, []);
  const governed = mode === "team";

  const targets = React.useMemo<NewCodingSessionTarget[]>(
    () =>
      resolveNewCodingSessionTargets({
        catalogs: providerCatalog.entries,
        channelId,
        localProvider: providerStatus?.providerPubkey
          ? {
              providerPubkey: providerStatus.providerPubkey,
              runtimes: providerRuntimes,
              modelsByInstanceRef: providerModelsByInstanceRef,
            }
          : null,
      }),
    [
      channelId,
      providerCatalog.entries,
      providerModelsByInstanceRef,
      providerRuntimes,
      providerStatus?.providerPubkey,
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
  // Re-read at click time: a cached target is a claim about a process that
  // may have exited while this screen was open.
  const refreshRuntimeTarget = React.useCallback(async () => {
    const refreshed = await refreshProviderState();
    return resolveSelectedNewCodingSessionTarget({
      targets: resolveNewCodingSessionTargets({
        catalogs: providerCatalog.entries,
        channelId,
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
    channelId,
    providerCatalog.entries,
    refreshProviderState,
    targetSelection.explicit,
    targetSelection.key,
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
  // Read only to learn which role each identity carries; the launch never
  // creates more than the lead's own seat.
  const roleHintsQuery = useQuery({
    queryKey: codingSessionCrewTeamsQueryKey,
    queryFn: listCodingSessionCrewTeams,
    staleTime: 30_000,
  });
  const roleHintRecords = React.useMemo<CodingSessionCrewTeam[]>(
    () => roleHintsQuery.data ?? [],
    [roleHintsQuery.data],
  );
  // What this project installed on this computer, from its setup journals:
  // those identities lead the picker, and a single installed `lead` is the
  // default. Scoped to the active relay so another community's installs never
  // attribute here.
  const { activeCommunity } = useCommunities();
  const installedRolesQuery = useProjectInstalledRolesQuery(
    activeCommunity?.relayUrl ?? null,
  );
  const installedRoles = React.useMemo(
    () => installedRolesForProject(installedRolesQuery.data, projectRef),
    [installedRolesQuery.data, projectRef],
  );
  const projectName =
    useCodingSessionProject(channelId, projectRef)?.name ?? null;
  const candidates = React.useMemo<NewCodingSessionLeadCandidate[]>(
    () =>
      applyCodingSessionInstalledRoles(
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
        installedRoles,
      ),
    [installedRoles, roleHintRecords, managedAgents],
  );
  const leadGroups = React.useMemo(
    () =>
      groupCodingSessionCandidates({
        candidates: candidates.filter((candidate) => candidate.role !== null),
        installedRoles,
        projectRef,
        projectName,
      }),
    [candidates, installedRoles, projectName, projectRef],
  );

  // Solo: you, whatever the Team picker last held. Team: the picked agent,
  // or `unset` — never "you" by fallback, which would seat a person as the
  // agent lead.
  //
  // An explicit pick is never overwritten; until there is one, the project's
  // single installed lead is preselected (Team only — Solo never reads it).
  const [leadSelection, setLeadSelection] = React.useState<{
    actor: string | null;
    explicit: boolean;
  }>({ actor: null, explicit: false });
  const setLeadActor = React.useCallback(
    (actor: string | null) => setLeadSelection({ actor, explicit: true }),
    [],
  );
  const leadActor = resolveCodingSessionLeadActor({
    selection: leadSelection,
    installedDefault: codingSessionInstalledLeadDefault({
      candidates,
      installedRoles,
    }),
  });
  const lead = React.useMemo<CodingSessionLaunchLead>(
    () =>
      mode === "solo"
        ? YOU_LEAD
        : resolveNewCodingSessionLead({ actor: leadActor, candidates }),
    [candidates, leadActor, mode],
  );

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
  // What the picker shows — about the runtime's catalog, not what is published.
  const effectiveModel = newCodingSessionEffectiveModel({
    seatModel: seatedModel,
    providerModel,
  });
  // What is published: the identity's own model or an explicit pick, never
  // the picker's default for an agent that declared none (REVIEW-B3 F1).
  const { model: leadModel, overridden: modelOverridden } =
    resolveCodingSessionLeadModel({
      lead,
      pickedModel: modelSelection.value,
      pickedExplicitly: modelSelection.explicit,
      providerModel: effectiveModel,
    });
  const [overrideReason, setOverrideReason] = React.useState("");

  const [benchIdentities, setBenchIdentities] = React.useState<string[]>([]);
  const [benchProviders, setBenchProviders] = React.useState<string[]>([]);
  const [challengerRate, setChallengerRate] = React.useState<number | null>(
    null,
  );
  const [policyDraft, setPolicyDraft] =
    React.useState<CodingSessionPolicyDraft>(EMPTY_CODING_SESSION_POLICY_DRAFT);
  // The bench IS policy: both halves ride the one 44245.
  const policy = React.useMemo<CodingSessionPolicyDraft>(
    () => ({
      ...policyDraft,
      benchIdentities,
      benchProviders,
      challengerSampleRate: challengerRate,
    }),
    [benchIdentities, benchProviders, challengerRate, policyDraft],
  );
  // Only a Team Start publishes a 44245. The draft survives a switch to Solo
  // so switching back restores it, but in Solo it is not set: the fields are
  // hidden, and a hidden value must not reach the wire or the plan.
  const policySet = governed && codingSessionPolicyDraftSetsAnything(policy);

  // What this computer recorded at the click, under the umbrella's ref:
  // where it runs, the workspace-reuse facts and the repository (LANE-L20:
  // resolved then, never guessed here). Nothing recorded (another desktop
  // founded it) leaves the field to its own prefill, and the card says so.
  const draft = React.useMemo(
    () => readCodingSessionFoundedDraft(sessionRef),
    [sessionRef],
  );
  const draftRef = React.useRef(draft);
  const [workdir, setWorkdir] = React.useState(draft?.workdir ?? "");
  const [useWorktree, setUseWorktree] = React.useState(
    draft?.useWorktree ?? true,
  );
  const [worktreeName, setWorktreeName] = React.useState(
    draft?.worktreeName ?? "",
  );
  const [worktreeSource, setWorktreeSource] = React.useState<string | null>(
    draft?.worktreeSource ?? null,
  );
  const workspaceReuse = React.useMemo<NewCodingSessionWorkspaceReuse | null>(
    () =>
      draft?.workspaceSourcePath
        ? {
            path: draft.workspaceSourcePath,
            branch: draft.workspaceSourceBranch,
            branchSource: draft.workspaceSourceBranchSource,
          }
        : null,
    [draft],
  );
  // The Name field's unpublished text lives in the same draft, so a name
  // typed and not yet committed survives a reload of this page.
  const [nameText, setNameText] = React.useState<string | null>(
    draft?.name ?? null,
  );
  const writeName = React.useCallback(
    (name: string | null) => {
      setNameText(name);
      const stored = draftRef.current ?? EMPTY_FOUNDED_DRAFT;
      const next = { ...stored, name };
      draftRef.current = next;
      writeCodingSessionFoundedDraft(sessionRef, next);
    },
    [sessionRef],
  );
  const nameDraft = React.useMemo(
    () => ({ name: nameText, setName: writeName }),
    [nameText, writeName],
  );
  const text = useCodingSessionFoundedText({
    channelId,
    sessionRef,
    wireName,
    namesResolved: nameResolved,
    nameReadError,
    refreshNames,
    goal,
    nameDraft,
    ...(textDeps ? { deps: textDeps } : {}),
  });

  // The roles this launch seats: the lead's plus every benched identity's.
  const launchRoles = React.useMemo(() => {
    const roles = new Set<string>();
    if (lead.kind === "agent") roles.add(lead.role);
    for (const pubkey of benchIdentities) {
      const role = candidates.find((entry) => entry.pubkey === pubkey)?.role;
      if (role) roles.add(role);
    }
    return [...roles].sort();
  }, [benchIdentities, candidates, lead]);
  const teamReadiness = useProjectTeamReadiness({
    projectRef,
    checkoutPath: workdir.trim().length > 0 ? workdir.trim() : null,
    selectedRoles: launchRoles,
    channelIds,
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
  // Roles are opt-in; off, the project-roles readiness neither gates nor shows.
  const [useRoles, setUseRoles] = React.useState(false);
  const readinessGate = teamReadinessLaunchGate({
    projectRef,
    loading: teamReadiness.isLoading,
    error: teamReadiness.readError,
    readiness: teamReadiness.readiness,
    runtimeTarget: selectedTarget,
    useRoles,
  });

  const crew = useCodingSessionCrewLaunch({
    // A session founded "in this workspace" keeps its folder out of the MRU.
    rememberWorkspace: draft?.rememberWorkspace ?? true,
    workdir: workdir.trim().length > 0 ? workdir.trim() : null,
    // Start flushes the Name field first; a title here would publish a second.
    title: null,
    policy: policySet ? policy : null,
  });
  const [launchError, setLaunchError] = React.useState<string | null>(null);
  const [setupError, setSetupError] = React.useState<string | null>(null);
  const [isPreparing, setIsPreparing] = React.useState(false);
  // Every busy state as the sentence a person reads, through readiness, so a
  // disabled Start is never silent (REVIEW-B3 F7).
  const busySentence = codingSessionFoundedBusySentence({
    starting,
    governed,
    channelSentence:
      channelReader === "loading"
        ? "Reading the channel's project…"
        : channelReader === "errored"
          ? "The channel could not be read, so whether this session belongs to a project is unknown; Start would sign a guess."
          : null,
    isPublishing: create.isPublishing,
    isLaunching: crew.isLaunching,
    isPreparing,
    transactionPending: create.transaction !== null,
    isLaunchPreflighting: teamReadiness.isLaunchPreflighting,
    isPreparingRoles: teamReadiness.isPreparing,
    isScanning: teamReadiness.isScanning,
  });
  const interactionLocked = busySentence !== null;

  const unresolvedBenchIdentities = React.useMemo(
    () =>
      benchIdentities.filter(
        (pubkey) => !candidates.some((entry) => entry.pubkey === pubkey),
      ),
    [benchIdentities, candidates],
  );
  const { readiness, plan } = codingSessionFoundedReadiness({
    mode,
    goal: text.prompt,
    goalOverflow: text.goalOverflow,
    goalReader: text.goalReader,
    nameResolved: nameResolved && !nameReadError,
    lead,
    providerInstanceRef: selectedTarget?.provider.providerInstanceRef ?? null,
    providerAuthorityPubkey: selectedTarget?.signerPubkey ?? null,
    leadModel,
    modelOverrideReason: overrideReason,
    modelOverridden,
    providerRefusal: newCodingSessionSeatModelBlocksCreate(seatedModel)
      ? seatedModel.note
      : null,
    projectRef,
    useRoles,
    readinessGate: {
      allowed: readinessGate.allowed,
      reason: readinessGate.reason,
    },
    teamReadinessLoading: teamReadiness.isLoading,
    teamReadinessError: teamReadiness.readError,
    policySet,
    benchCount: benchIdentities.length,
    unresolvedBenchIdentities,
    busySentence,
    nameDirty: text.nameDirty,
    promptDirty: text.promptDirty,
    useWorktree,
    worktreeName,
  });

  // A press that was refused for something surfaced on press (no lead, no
  // worktree name, a blank prompt) turns the refused fields' sentences on
  // and, in Team, the launch plan; a change to any of those fields turns
  // them off again until the next press.
  const [attempted, setAttempted] = React.useState(false);
  const markAttempted = React.useCallback(() => setAttempted(true), []);
  const attemptedFor = React.useRef({
    mode,
    leadActor,
    useWorktree,
    worktreeName,
  });
  React.useEffect(() => {
    const previous = attemptedFor.current;
    if (
      previous.mode !== mode ||
      previous.leadActor !== leadActor ||
      previous.useWorktree !== useWorktree ||
      previous.worktreeName !== worktreeName
    ) {
      attemptedFor.current = { mode, leadActor, useWorktree, worktreeName };
      setAttempted(false);
    }
  }, [mode, leadActor, useWorktree, worktreeName]);

  const status = newCodingSessionStatusMessage({
    hostPhase: create.hostPhase,
    isPublishing: create.isPublishing,
    publishError: create.publishError ?? create.durabilityError,
    lifecycle: create.lifecycle,
    authRuntime: null,
    stalled: create.stalled,
    publishState: create.transaction?.publishState ?? null,
  });
  const failureCode =
    create.lifecycle?.state === "failed"
      ? create.lifecycle.error.code
      : undefined;

  const benchGroups = groupCodingSessionCandidates({
    candidates: candidates.filter(
      (candidate) =>
        candidate.role !== null &&
        !(lead.kind === "agent" && candidate.pubkey === lead.actor),
    ),
    installedRoles,
    projectRef,
    projectName,
  });
  // Project roles first, under the same headings as the lead picker.
  const benchIdentityOptions: NewCodingSessionBenchOption[] =
    benchGroups.flatMap((group) =>
      group.candidates.map((candidate) => ({
        value: candidate.pubkey,
        label: candidate.name,
        detail: candidate.role,
        group: group.heading,
      })),
    );
  const benchProviderOptions: NewCodingSessionBenchOption[] = targets.map(
    (target) => ({
      value: target.provider.providerInstanceRef,
      label: target.availability?.label ?? target.provider.runtime,
      detail: null,
    }),
  );

  return {
    mode,
    setMode,
    text,
    channelReader,
    targets,
    selectedTarget,
    selectTarget: (key: string) => {
      setTargetSelection({ key, explicit: true });
      setModelSelection({ value: null, explicit: false });
    },
    refreshRuntimeTarget,
    modelCatalog,
    effectiveModel,
    seatedModelNote: seatedModel.note,
    selectModel: (value: string) =>
      setModelSelection({ value, explicit: true }),
    leadModel,
    modelOverridden,
    overrideReason,
    setOverrideReason,
    candidates,
    /** "Who leads" options: the project's installed roles first. */
    leadGroups,
    lead,
    setLeadActor,
    governed,
    benchIdentityOptions,
    benchProviderOptions,
    benchIdentities,
    benchProviders,
    toggleBenchIdentity: (value: string, selected: boolean) =>
      setBenchIdentities((previous) =>
        selected
          ? [...new Set([...previous, value])]
          : previous.filter((entry) => entry !== value),
      ),
    toggleBenchProvider: (value: string, selected: boolean) =>
      setBenchProviders((previous) =>
        selected
          ? [...new Set([...previous, value])]
          : previous.filter((entry) => entry !== value),
      ),
    challengerRate,
    setChallengerRate,
    policyDraft,
    setPolicyDraft,
    policySet,
    /** Whether this computer recorded where this session runs, at the click. */
    draftSource: (draft === null
      ? "none"
      : draft.workdir !== null
        ? "workdir"
        : "empty") as "workdir" | "empty" | "none",
    /** The click-time facts Start signs from: never guessed on this screen. */
    draft: {
      rememberWorkspace: draft?.rememberWorkspace ?? true,
      repoRef: draft?.repoRef ?? null,
      workspaceSourcePath: draft?.workspaceSourcePath ?? null,
    },
    workspaceReuse,
    workdir,
    setWorkdir,
    useWorktree,
    setUseWorktree,
    worktreeName,
    setWorktreeName,
    worktreeSource,
    setWorktreeSource,
    useRoles,
    setUseRoles,
    launchRoles,
    teamReadiness,
    beginLoginWatch: create.beginLoginWatch,
    submit: create.submit,
    startFresh: create.startFresh,
    startFreshReadiness: create.startFreshReadiness,
    transaction: create.transaction,
    lifecycleState: create.lifecycle?.state ?? null,
    failureCode,
    status,
    launch: crew.launch,
    isLaunching: crew.isLaunching,
    isPublishing: create.isPublishing,
    steps: crew.steps,
    launchResult: crew.result,
    readiness,
    plan,
    canLaunch: readiness.canLaunch,
    attempted,
    markAttempted,
    /**
     * What the worktree field derives its name from: the Name, or, with the
     * Name blank (it is optional now), the session's ref — unique, and
     * visibly editable, rather than a blank the cut would refuse.
     */
    worktreeSeed: text.name.trim() || `session-${sessionRef.slice(0, 8)}`,
    interactionLocked,
    launchError,
    setLaunchError,
    setupError,
    setSetupError,
    setIsPreparing,
  };
}

export type CodingSessionFoundedSetupModel = ReturnType<
  typeof useCodingSessionFoundedSetup
>;
