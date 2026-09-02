import { useQuery } from "@tanstack/react-query";
import { CircleAlert, CircleCheck, LoaderCircle, Users } from "lucide-react";
import * as React from "react";

import { useManagedAgentsQuery } from "@/features/agents/hooks";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Textarea } from "@/shared/ui/textarea";
import { cn } from "@/shared/lib/cn";
import {
  checkCodingSessionCrewFamilies,
  checkCodingSessionCrewSeatModels,
  codingSessionCrewLaunchBlock,
  describeCodingSessionSeatVendor,
  resolveCodingSessionSeatVendor,
  type ResolvedCodingSessionCrewSeat,
} from "../lib/codingSessionCrew";
import {
  codingSessionCrewLeadDestination,
  codingSessionCrewProjectNote,
  type CodingSessionCrewLaunchStep,
} from "../lib/codingSessionCrewLaunch";
import {
  codingSessionGoalOverflow,
  codingSessionGoalOverflowSentence,
  MAX_CODING_SESSION_GOAL_BYTES,
} from "../lib/codingSessionGoal";
import { codingSessionLeadWorktreeName } from "../lib/codingSessionWorktreeName";
import {
  isNewCodingSessionTargetReady,
  type NewCodingSessionTarget,
} from "../lib/newCodingSessionModel";
import {
  listCodingSessionCrewTeams,
  resolveCodingSessionCrewSeats,
  type CodingSessionCrewTeam,
} from "../lib/codingSessionCrewTeams";
import {
  normalizeTeamReadinessRoles,
  teamReadinessLaunchGate,
  type TeamReadinessLaunchGate,
} from "../lib/teamReadinessModel";
import { useProjectTeamReadiness } from "../lib/useProjectTeamReadiness";
import { NewCodingSessionModelDisclosure } from "./NewCodingSessionProviderPicker";
import { NewCodingSessionWorkdirField } from "./NewCodingSessionWorkdirField";
import { NewCodingSessionWorktreeField } from "./NewCodingSessionWorktreeField";
import { TeamReadinessCard } from "./TeamReadinessCard";
import { useCodingSessionCrewLaunch } from "./useCodingSessionCrewLaunch";
import { codingSessionCreateModelLabel } from "./useNewCodingSessionCreate";

export const codingSessionCrewTeamsQueryKey = ["coding-session-crew-teams"];

/** Select the lead this launch creates, with the primary as the sole fallback. */
export function codingSessionCrewLeadSeat<
  Seat extends { personaId: string; role: string },
>(seats: readonly Seat[], primaryPersonaId: string): Seat | null {
  return (
    seats.find((seat) => seat.role.trim().toLowerCase() === "lead") ??
    seats.find((seat) => seat.personaId === primaryPersonaId) ??
    null
  );
}

/** Canonicalize the selected explicit lead for the downstream launch selector. */
export function codingSessionCrewLaunchSeats(
  seats: readonly ResolvedCodingSessionCrewSeat[],
  lead: ResolvedCodingSessionCrewSeat | null,
): ResolvedCodingSessionCrewSeat[] {
  if (lead?.role.trim().toLowerCase() !== "lead") return [...seats];
  return seats.map((seat) =>
    seat.personaId === lead.personaId ? { ...seat, role: "lead" } : seat,
  );
}

/** Read the one role this launch creates from the team definition. */
export function codingSessionCrewReadinessRoles(
  team: CodingSessionCrewTeam | null,
): string[] {
  if (!team) return [];
  const lead = codingSessionCrewLeadSeat(team.crew.seats, team.crew.primary);
  return normalizeTeamReadinessRoles(lead ? [lead.role] : []);
}

/** Keep the cached readiness gate and the rendered Launch state identical. */
export function codingSessionCrewLaunchEnabled(input: {
  cachedReadinessAllowed: boolean;
  interactionLocked: boolean;
  launchBlock: string | null;
  refusal: string | null;
}): boolean {
  return (
    input.cachedReadinessAllowed &&
    input.refusal === null &&
    input.launchBlock === null &&
    !input.interactionLocked
  );
}

/** Re-read both launch authorities at click time; cached UI state is never enough. */
export async function codingSessionCrewFreshLaunchGate(input: {
  projectRef: string | null;
  refreshRuntimeTarget: () => Promise<NewCodingSessionTarget | null>;
  readFreshReadiness: () => Promise<
    Awaited<
      ReturnType<
        ReturnType<typeof useProjectTeamReadiness>["readFreshForLaunch"]
      >
    >
  >;
}): Promise<{
  gate: TeamReadinessLaunchGate;
  runtimeTarget: NewCodingSessionTarget | null;
}> {
  const runtimeTarget = await input.refreshRuntimeTarget();
  const readiness = await input.readFreshReadiness();
  return {
    gate: teamReadinessLaunchGate({
      projectRef: input.projectRef,
      loading: false,
      error: null,
      readiness,
      runtimeTarget,
    }),
    runtimeTarget,
  };
}

/** Validate only the seat this provider will actually create. */
export function codingSessionCrewProviderRefusal(input: {
  lead: ResolvedCodingSessionCrewSeat | null;
  provider: {
    allowedModels: readonly string[];
    instanceRef: string | null;
    label: string | null;
  };
}): string | null {
  if (!input.lead) {
    return "This team has no lead seat to hold operator authority.";
  }
  const createdSeats = [input.lead];
  const runnable = checkCodingSessionCrewSeatModels(
    createdSeats,
    input.provider,
  );
  if (!runnable.ok) return runnable.reason;
  const family = checkCodingSessionCrewFamilies(createdSeats);
  return family.ok ? null : family.reason;
}

/**
 * What Launch actually does, said before it is pressed.
 *
 * D14 makes the launch a front door rather than a batch: the lead hears the
 * mission and hires the team itself, one brief at a time. A screen listing
 * four roles beside a button called "Launch team" implies four agents start —
 * so the sentence names the one that does, and the verb that brings the rest.
 */
export const CODING_SESSION_CREW_LAUNCH_SCOPE_NOTE =
  "Launching seats the lead only. The roles below are who it may hire — it " +
  "hires them with `bee sessions hire` once it knows what the work is, each " +
  "one on whatever provider that seat needs.";

/**
 * The sentence under the team select: which runtime the lead runs on, and on
 * which model.
 *
 * The team tab has no model picker of its own (item 87c), so this line is the
 * only place the model appears before the launch — which made it the place
 * where `default` was printed as if it were one. When the runtime's own
 * default is that id, the sentence says what it is instead of repeating the
 * token, and {@link NewCodingSessionModelDisclosure} beneath it says what the
 * record will therefore not contain.
 */
export function codingSessionCrewProviderNote(input: {
  providerLabel: string | null;
  model: string | null;
  allowedModels: readonly string[];
}): string {
  const label = codingSessionCreateModelLabel({
    model: input.model,
    catalog: {
      // The tab is handed the model the dialog already resolved from this
      // runtime's catalog, so it *is* that catalog's default.
      defaultModel: input.model ?? "",
      allowedModels: [...input.allowedModels],
    },
  });
  return (
    `${CODING_SESSION_CREW_LAUNCH_SCOPE_NOTE} The lead runs on ${
      input.providerLabel ?? "this computer's provider"
    }` +
    `${label ? `, on ${label} unless its seat names its own` : ""}, ` +
    "in the directory below."
  );
}

/**
 * Everything the goal field owes the person: its size, its cap, and what the
 * launch did about it.
 *
 * Three sentences that used to be missing. The cap is in UTF-8 bytes and the
 * field counts characters, so without a byte counter a person can type a goal
 * the signer will refuse and see nothing until the button goes dead — and an
 * over-cap goal used to launch a whole team and quietly publish no kind:44227
 * (item 103 finding 5; batch 2 review A2 F1). At the cap the counter *becomes*
 * the refusal, in the launch block's own words, so the two cannot drift. After
 * a launch, a goal that did not go out says so here rather than showing up as
 * an empty goal pill in the session, which reads as "nobody set one".
 */
export function CodingSessionCrewGoalNotes({
  bytes,
  goalOutcome,
  overflow,
}: {
  /** The trimmed goal's size in UTF-8 bytes — the unit the cap is in. */
  bytes: number;
  /** What the last launch did about the goal, or null before one. */
  goalOutcome: { published: boolean; reason: string | null } | null;
  /** The overflow the shared helper reports, or null when the goal fits. */
  overflow: { bytes: number; cap: number } | null;
}) {
  return (
    <>
      {overflow === null ? (
        <p
          className="text-2xs text-muted-foreground"
          data-testid="new-coding-session-crew-goal-bytes"
        >
          {`${bytes.toLocaleString()} of ${MAX_CODING_SESSION_GOAL_BYTES.toLocaleString()} UTF-8 bytes.`}{" "}
          The lead's first turn carries this goal, and the launch publishes it
          as the session's own goal.
        </p>
      ) : (
        <p
          className="text-2xs text-destructive"
          data-testid="new-coding-session-crew-goal-bytes"
        >
          {codingSessionGoalOverflowSentence(overflow)}
        </p>
      )}
      {goalOutcome !== null && !goalOutcome.published ? (
        <p
          className="text-2xs text-destructive"
          data-testid="new-coding-session-crew-goal-unpublished"
          role="alert"
        >
          {`The team launched, but its goal was not published: ${
            goalOutcome.reason ?? "the goal publish did not go out"
          }. Set it from the session's goal pill.`}
        </p>
      ) : null}
    </>
  );
}

/**
 * Launch a team into one session: pick the team, the repo, and the goal.
 *
 * The refusals this tab is built around are all *before* anything is signed,
 * and all say what to do. Provider/model checks apply to the lead alone,
 * because that is the only seat this launch creates; the lead routes every
 * later hire independently. The launch button stays disabled and the reason
 * is on screen — never a launch that half-happens and explains itself later.
 */
export function NewCodingSessionCrewTab({
  channelId,
  defaultWorkdir = null,
  disabled,
  ensureChannelId = null,
  onLaunched,
  projectName = null,
  projectRef = null,
  providerAuthorityPubkey,
  providerInstanceRef,
  providerLabel,
  providerAllowedModels,
  runtimeTarget,
  refreshRuntimeTarget,
  model,
}: {
  channelId: string | null;
  /** The project's local checkout, when the dialog knows one. */
  defaultWorkdir?: string | null;
  disabled: boolean;
  /**
   * Resolve — publishing it if this is its first session — the channel the
   * team launches into. Supplied by the project flow, whose sessions channel
   * has no id until the first create needs one; without it a null `channelId`
   * is a refusal, because there is nowhere to publish and nothing that could
   * make one.
   */
  ensureChannelId?: (() => Promise<string>) | null;
  /**
   * Where to go once the lead is seated: the channel it landed in and, when
   * the receipt resolved one, the exact generation to open.
   */
  onLaunched: (input: {
    channelId: string;
    generationId: string | null;
  }) => void;
  /** The project this launch belongs to, named on the tab before it runs. */
  projectName?: string | null;
  /**
   * The project coordinate signed into the lead's create. Null is a real
   * answer and the tab says so rather than letting the session land nowhere.
   */
  projectRef?: string | null;
  providerAuthorityPubkey: string | null;
  providerInstanceRef: string | null;
  /** Name of the runtime the lead will run on, for the disclosure line. */
  providerLabel: string | null;
  /**
   * Models the selected provider runtime actually publishes.
   *
   * The vendor rule is decided on a model, and a model this runtime does not
   * have is replaced by its default — so without this list the check reads a
   * string nothing verified. An empty list is a refusal, not a pass.
   */
  providerAllowedModels: readonly string[];
  /** The exact installed/authenticated target selected by the create picker. */
  runtimeTarget: NewCodingSessionTarget | null;
  refreshRuntimeTarget: () => Promise<NewCodingSessionTarget | null>;
  model: string | null;
}) {
  const crewTeamsQuery = useQuery({
    queryKey: codingSessionCrewTeamsQueryKey,
    queryFn: listCodingSessionCrewTeams,
    staleTime: 30_000,
  });
  const crewTeams = React.useMemo<CodingSessionCrewTeam[]>(
    () => crewTeamsQuery.data ?? [],
    [crewTeamsQuery.data],
  );
  const managedAgentsQuery = useManagedAgentsQuery();
  const [teamId, setTeamId] = React.useState<string | null>(null);
  const [goal, setGoal] = React.useState("");
  const [title, setTitle] = React.useState("");
  const [workdir, setWorkdir] = React.useState("");
  // On by default, for the same reason the one-session path defaults it on:
  // the alternative is the lead sharing one index, one HEAD and one
  // `.agents/skills` with whoever else has that checkout open — which is
  // exactly what happened to the operator's own checkout (item 87d).
  const [useWorktree, setUseWorktree] = React.useState(true);
  const [worktreeName, setWorktreeName] = React.useState("");
  const [worktreeSource, setWorktreeSource] = React.useState<string | null>(
    null,
  );
  const [launchError, setLaunchError] = React.useState<string | null>(null);

  const selectedTeam =
    crewTeams.find((team) => team.id === teamId) ?? crewTeams[0] ?? null;

  const resolution = React.useMemo(() => {
    if (!selectedTeam) return null;
    return resolveCodingSessionCrewSeats({
      crew: selectedTeam.crew,
      agents: (managedAgentsQuery.data ?? []).map((agent) => ({
        pubkey: agent.pubkey,
        name: agent.name,
        personaId: agent.personaId,
        model: agent.model,
        hasRolePack: agent.hasRolePack,
      })),
      // The same fallback the create would otherwise apply on its own, moved
      // here so the vendor rule is checked against the published model.
      fallbackModel: model,
    });
  }, [managedAgentsQuery.data, model, selectedTeam]);

  const seats = resolution?.seats ?? null;
  const selectedLead =
    seats && selectedTeam
      ? codingSessionCrewLeadSeat(seats, selectedTeam.crew.primary)
      : null;
  const launchSeats = seats
    ? codingSessionCrewLaunchSeats(seats, selectedLead)
    : null;
  const launchPrimaryPersonaId =
    selectedLead?.personaId ?? selectedTeam?.crew.primary ?? "";
  const selectedRoles = React.useMemo(
    () => codingSessionCrewReadinessRoles(selectedTeam),
    [selectedTeam],
  );
  const teamReadiness = useProjectTeamReadiness({
    projectRef,
    checkoutPath: defaultWorkdir,
    selectedRoles,
    // A catalog from another membership channel says nothing about the
    // session being launched here. Before the first channel exists, native
    // readiness preserves the explicit awaiting-first-session state.
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
  const cachedReadinessGate = teamReadinessLaunchGate({
    projectRef,
    loading: teamReadiness.isLoading,
    error: teamReadiness.readError,
    readiness: teamReadiness.readiness,
    runtimeTarget,
  });
  const provider = React.useMemo(
    () => ({
      allowedModels: providerAllowedModels,
      // The runtime the lead is created against. Future seats are routed when
      // the lead hires them and need not share this provider.
      instanceRef: providerInstanceRef,
      label: providerLabel,
    }),
    [providerAllowedModels, providerInstanceRef, providerLabel],
  );
  // Match codingSessionCrewLaunch exactly: this provider creates only the
  // lead. Future seats remain provider-neutral until their signed hire.
  const providerRefusal =
    seats && selectedTeam
      ? codingSessionCrewProviderRefusal({
          lead: selectedLead,
          provider,
        })
      : null;
  // Only about the team that is actually selected: with no team to launch,
  // a missing provider is not yet anybody's problem to read.
  const refusal =
    selectedTeam === null
      ? null
      : ((providerInstanceRef === null || providerAuthorityPubkey === null
          ? "No coding-session provider is available on this computer, so there is nothing to run the team on."
          : null) ??
        resolution?.error ??
        providerRefusal);

  // The worktree field slugs whatever it is handed, and the slug is
  // idempotent — so handing it the already-suffixed name is what makes the
  // prefill read `<session>-lead` and keep following the session's name.
  const leadWorktreeSuggestion = codingSessionLeadWorktreeName(
    title.trim() || selectedTeam?.name || "",
  );

  const { isLaunching, launch, result, steps } = useCodingSessionCrewLaunch({
    ensureChannelId,
    workdir: workdir.trim().length > 0 ? workdir.trim() : null,
    title: title.trim().length > 0 ? title.trim() : null,
  });
  const interactionLocked =
    disabled ||
    isLaunching ||
    teamReadiness.isPreparing ||
    teamReadiness.isScanning ||
    teamReadiness.isLaunchPreflighting;

  // The goal's size in the unit the cap is in. Measured on the trimmed text by
  // the same helper the launch block and the signer use, so the counter under
  // the field can never disagree with the refusal under the button.
  const goalBytes = React.useMemo(
    () => new TextEncoder().encode(goal.trim()).byteLength,
    [goal],
  );
  const goalOverflow = React.useMemo(
    () => codingSessionGoalOverflow(goal),
    [goal],
  );

  // Every reason the button is off, in one sentence — and the same expression
  // the button is disabled on, so a disabled control can never be silent.
  const launchBlock = codingSessionCrewLaunchBlock({
    hasTeam: selectedTeam !== null,
    seatCount: seats?.length ?? null,
    hasChannel: channelId !== null,
    canCreateChannel: ensureChannelId !== null,
    createInFlight: disabled,
    isLaunching,
    goal,
  });
  const canLaunch = codingSessionCrewLaunchEnabled({
    cachedReadinessAllowed: cachedReadinessGate.allowed,
    interactionLocked,
    launchBlock,
    refusal,
  });

  const readFreshForLaunch = teamReadiness.readFreshForLaunch;
  const handleLaunch = React.useCallback(() => {
    if (!canLaunch || !selectedTeam || !launchSeats) return;
    setLaunchError(null);
    void (async () => {
      try {
        const fresh = await codingSessionCrewFreshLaunchGate({
          projectRef,
          refreshRuntimeTarget,
          readFreshReadiness: readFreshForLaunch,
        });
        if (!fresh.gate.allowed || fresh.runtimeTarget === null) {
          setLaunchError(
            `Launch blocked by fresh Team Readiness: ${fresh.gate.reason ?? "the project is not prepared"}`,
          );
          return;
        }
        const freshProvider = {
          allowedModels: fresh.runtimeTarget.provider.allowedModels,
          instanceRef: fresh.runtimeTarget.provider.providerInstanceRef,
          label: fresh.runtimeTarget.availability?.label ?? null,
        };
        const freshProviderRefusal = codingSessionCrewProviderRefusal({
          lead: selectedLead,
          provider: freshProvider,
        });
        if (freshProviderRefusal) {
          setLaunchError(
            `Launch blocked by refreshed provider: ${freshProviderRefusal}`,
          );
          return;
        }
        const result = await launch(
          {
            channelId,
            goal,
            seats: launchSeats,
            primaryPersonaId: launchPrimaryPersonaId,
            projectRef,
            provider: freshProvider,
            workdir: workdir.trim().length > 0 ? workdir.trim() : null,
            leadWorktree:
              useWorktree && worktreeName.trim().length > 0
                ? { name: worktreeName.trim(), source: worktreeSource }
                : null,
          },
          fresh.runtimeTarget,
        );
        // The channel the launch settled on: for a project's first session it
        // is the one the launch just published, not the null it was handed.
        if (result.ok && result.channelId) {
          onLaunched({
            channelId: result.channelId,
            // The lead's own generation, resolved from its receipt — a launch
            // that closed the dialog and navigated nowhere is why item 87
            // exists.
            generationId:
              codingSessionCrewLeadDestination({
                result,
                providerAuthorityPubkey: fresh.runtimeTarget.signerPubkey,
              })?.generationId ?? null,
          });
        } else if (result.ok) {
          setLaunchError(
            "The team launched, but into no channel this screen can name.",
          );
        } else setLaunchError(result.failureReason);
      } catch (error) {
        setLaunchError(
          error instanceof Error
            ? error.message
            : "The team could not be launched.",
        );
      }
    })();
  }, [
    canLaunch,
    channelId,
    goal,
    launch,
    launchPrimaryPersonaId,
    launchSeats,
    onLaunched,
    projectRef,
    readFreshForLaunch,
    refreshRuntimeTarget,
    selectedLead,
    selectedTeam,
    useWorktree,
    workdir,
    worktreeName,
    worktreeSource,
  ]);

  return (
    <div className="flex flex-col gap-5" data-testid="new-coding-session-crew">
      <div className="flex flex-col gap-2">
        <label
          className="text-xs font-medium text-muted-foreground"
          htmlFor="coding-session-crew-team"
        >
          Team
        </label>
        <select
          className="h-9 rounded-md border border-input bg-transparent px-3 text-sm disabled:opacity-50"
          data-testid="new-coding-session-crew-team"
          disabled={interactionLocked || crewTeams.length === 0}
          id="coding-session-crew-team"
          onChange={(event) => setTeamId(event.target.value)}
          value={selectedTeam?.id ?? ""}
        >
          {crewTeams.length === 0 ? (
            <option value="">No teams with seats on this computer</option>
          ) : null}
          {crewTeams.map((team) => (
            <option key={team.id} value={team.id}>
              {team.name}
            </option>
          ))}
        </select>
        <p className="text-2xs text-muted-foreground">
          {crewTeams.length === 0
            ? "A launchable team is one whose seats carry roles. None of this computer's teams do yet."
            : // The one-provider limitation that used to block a mixed-vendor
              // roster (item 79c/79e) applies to the lead alone now, because
              // the lead is the only seat this launch creates. Each later seat
              // picks its own runtime when the lead hires it.
              codingSessionCrewProviderNote({
                allowedModels: providerAllowedModels,
                model,
                providerLabel,
              })}
        </p>
        {crewTeams.length === 0 ? null : (
          <NewCodingSessionModelDisclosure
            catalog={{
              defaultModel: model ?? "",
              allowedModels: [...providerAllowedModels],
            }}
            model={model}
          />
        )}
      </div>

      <p
        className="text-2xs text-muted-foreground"
        data-testid="new-coding-session-crew-project"
      >
        {codingSessionCrewProjectNote(projectName)}
      </p>

      {projectRef ? (
        <TeamReadinessCard
          loading={teamReadiness.isLoading}
          externalBusy={
            disabled || isLaunching || teamReadiness.isLaunchPreflighting
          }
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
          selectedRoles={selectedRoles}
          runtimeTarget={runtimeTarget}
        />
      ) : null}

      {selectedTeam ? (
        <CodingSessionCrewRoster
          primaryPersonaId={launchPrimaryPersonaId}
          seats={launchSeats}
        />
      ) : null}

      {refusal ? (
        <p
          className="flex items-start gap-2 text-sm text-destructive"
          data-testid="new-coding-session-crew-refusal"
          role="alert"
        >
          <CircleAlert className="mt-0.5 size-4 shrink-0" />
          {refusal}
        </p>
      ) : launchBlock || cachedReadinessGate.reason ? (
        // Not destructive: nothing is wrong, something is missing. But never
        // silent — a disabled Launch with no sentence under it is the front
        // door refusing without saying why (item 79).
        <p
          className="text-2xs text-muted-foreground"
          data-testid="new-coding-session-crew-blocked"
        >
          {launchBlock ?? cachedReadinessGate.reason}
        </p>
      ) : null}

      <div className="flex flex-col gap-2">
        <label
          className="text-xs font-medium text-muted-foreground"
          htmlFor="coding-session-crew-goal"
        >
          Goal
        </label>
        <Textarea
          className="min-h-32"
          data-testid="new-coding-session-crew-goal"
          disabled={interactionLocked}
          id="coding-session-crew-goal"
          // The cap is in UTF-8 bytes, so no character count can enforce it —
          // this is only the ceiling no legal goal can be cut by (a character
          // is never fewer than one byte), matching the goal pill's own bound.
          // The byte counter below is what actually tells the truth.
          maxLength={MAX_CODING_SESSION_GOAL_BYTES}
          onChange={(event) => setGoal(event.target.value)}
          placeholder="What is this team for?"
          value={goal}
        />
        <CodingSessionCrewGoalNotes
          bytes={goalBytes}
          goalOutcome={result?.goal ?? null}
          overflow={goalOverflow}
        />
      </div>

      <div className="flex flex-col gap-2">
        <label
          className="text-xs font-medium text-muted-foreground"
          htmlFor="coding-session-crew-title"
        >
          Name <span className="font-normal">(optional)</span>
        </label>
        <Input
          data-testid="new-coding-session-crew-title"
          disabled={interactionLocked}
          id="coding-session-crew-title"
          onChange={(event) => setTitle(event.target.value)}
          placeholder="What is this session for?"
          value={title}
        />
      </div>

      <NewCodingSessionWorkdirField
        channelId={channelId}
        disabled={interactionLocked}
        fallbackPath={defaultWorkdir}
        onChange={setWorkdir}
        projectKey={projectRef}
        value={workdir}
      />

      <NewCodingSessionWorktreeField
        checked={useWorktree}
        disabled={interactionLocked}
        name={worktreeName}
        onCheckedChange={setUseWorktree}
        onNameChange={setWorktreeName}
        onSourceChange={setWorktreeSource}
        sessionName={leadWorktreeSuggestion}
        source={worktreeSource}
        workdir={workdir}
      />

      {steps.length > 0 ? <CodingSessionCrewLaunchSteps steps={steps} /> : null}

      <CodingSessionCrewSkillNotice
        labels={result?.seatsWithoutRolePack ?? []}
      />

      {launchError ? (
        <p
          className="flex items-start gap-2 text-sm text-destructive"
          data-testid="new-coding-session-crew-error"
          role="alert"
        >
          <CircleAlert className="mt-0.5 size-4 shrink-0" />
          {launchError}
        </p>
      ) : null}

      <p className="text-2xs text-muted-foreground">
        A team launch is not resumable. If it stops partway, whatever it already
        published stays — finish from the session itself.
      </p>

      <div className="flex shrink-0 items-center justify-end">
        <Button
          data-testid="new-coding-session-crew-launch"
          disabled={!canLaunch}
          onClick={handleLaunch}
          type="button"
        >
          {isLaunching || teamReadiness.isLaunchPreflighting ? (
            <LoaderCircle className="animate-spin motion-reduce:animate-none" />
          ) : (
            <Users />
          )}
          {teamReadiness.isLaunchPreflighting
            ? "Checking readiness…"
            : "Launch team"}
        </Button>
      </div>
    </div>
  );
}

/**
 * The seats: which one this launch creates, and which the lead may hire.
 *
 * The distinction is rendered, not implied. Before D14 every row looked
 * identical and every row was created; now exactly one is, and a roster that
 * still read as a manifest would tell the same lie on screen that the launch
 * used to tell in events — four rows, one live agent.
 */
export function CodingSessionCrewRoster({
  primaryPersonaId,
  seats,
}: {
  primaryPersonaId: string;
  seats: ResolvedCodingSessionCrewSeat[] | null;
}) {
  if (!seats) return null;
  const lead = codingSessionCrewLeadSeat(seats, primaryPersonaId);
  return (
    <ul
      className="flex flex-col gap-1 rounded-lg border border-border/60 bg-muted/30 px-3 py-2.5"
      data-testid="new-coding-session-crew-roster"
    >
      {seats.map((seat) => {
        const vendor = resolveCodingSessionSeatVendor(seat);
        const seated = lead !== null && seat.personaId === lead.personaId;
        return (
          <li
            className="flex items-baseline gap-2 text-sm"
            data-seat-state={seated ? "seated" : "hireable"}
            key={`${seat.personaId}:${seat.actor}`}
          >
            <span className="font-medium">{seat.role}</span>
            <span className="truncate">{seat.actorLabel}</span>
            <span
              className={cn(
                "text-2xs",
                vendor.vendor === null
                  ? "text-destructive"
                  : "text-muted-foreground",
              )}
            >
              {describeCodingSessionSeatVendor(seat, { annotateSource: true })}
              {seat.model && vendor.source !== "conflict"
                ? ` · ${seat.model}`
                : ""}
            </span>
            <span className="text-2xs text-muted-foreground">
              {seated
                ? "· seated on launch, first turn"
                : "· not launched — the lead may hire it"}
            </span>
            {/* Before the launch, not after it. The launch's own
                `seatsWithoutRolePack` only exists once staging has answered,
                which is after the seats are signed for; the agent already told
                us here. `undefined` says nothing — nobody asked. */}
            {seat.hasRolePack === false ? (
              <span
                className="text-2xs text-amber-600 dark:text-amber-400"
                data-testid={`crew-seat-no-role-pack-${seat.personaId}`}
              >
                · carries no role skills: this computer has no role pack behind
                it.
              </span>
            ) : null}
          </li>
        );
      })}
    </ul>
  );
}

/**
 * What the seats that got no role pack actually carry.
 *
 * A team tab that lists six roles and then says nothing implies six seats
 * holding their roles' craft. When this computer has no pack behind a seat's
 * persona, nothing wrote `.agents/skills` into its working directory, and this
 * line is the difference between a seat that lacks craft and a screen that
 * lies about it.
 */
export function CodingSessionCrewSkillNotice({
  labels,
}: {
  labels: readonly string[];
}) {
  if (labels.length === 0) return null;
  return (
    <p
      className="text-2xs text-muted-foreground"
      data-testid="new-coding-session-crew-no-skills"
    >
      {labels.join(", ")} {labels.length === 1 ? "carries" : "carry"} no role
      skills: this computer has no role pack behind{" "}
      {labels.length === 1 ? "that persona" : "those personas"}, so nothing was
      written to <code>.agents/skills</code> in the working directory. The
      {labels.length === 1 ? " seat runs" : " seats run"} on the persona prompt
      alone.
    </p>
  );
}

/** The signed sequence, one row per step, with the failed one named. */
export function CodingSessionCrewLaunchSteps({
  steps,
}: {
  steps: CodingSessionCrewLaunchStep[];
}) {
  return (
    <ol
      className="flex flex-col gap-1"
      data-testid="new-coding-session-crew-steps"
    >
      {steps.map((step) => (
        <li
          className={cn(
            "flex items-start gap-2 text-2xs",
            step.state === "failed"
              ? "text-destructive"
              : step.state === "done"
                ? "text-muted-foreground"
                : "text-muted-foreground/70",
          )}
          data-state={step.state}
          data-testid={`crew-step-${step.id}`}
          key={step.id}
        >
          {step.state === "running" ? (
            <LoaderCircle className="mt-0.5 size-3 shrink-0 animate-spin motion-reduce:animate-none" />
          ) : step.state === "done" ? (
            <CircleCheck className="mt-0.5 size-3 shrink-0" />
          ) : step.state === "failed" ? (
            <CircleAlert className="mt-0.5 size-3 shrink-0" />
          ) : (
            <span className="mt-0.5 size-3 shrink-0" />
          )}
          <span>
            {step.label}
            {step.detail ? ` — ${step.detail}` : ""}
          </span>
        </li>
      ))}
    </ol>
  );
}
