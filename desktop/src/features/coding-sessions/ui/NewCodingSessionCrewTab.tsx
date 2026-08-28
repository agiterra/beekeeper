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
  describeCodingSessionSeatVendor,
  resolveCodingSessionSeatVendor,
  type ResolvedCodingSessionCrewSeat,
} from "../lib/codingSessionCrew";
import type { CodingSessionCrewLaunchStep } from "../lib/codingSessionCrewLaunch";
import {
  listCodingSessionCrewTeams,
  resolveCodingSessionCrewSeats,
  type CodingSessionCrewTeam,
} from "../lib/codingSessionCrewTeams";
import { NewCodingSessionWorkdirField } from "./NewCodingSessionWorkdirField";
import { useCodingSessionCrewLaunch } from "./useCodingSessionCrewLaunch";

export const codingSessionCrewTeamsQueryKey = ["coding-session-crew-teams"];

/**
 * Launch a crew into one session: pick the crew, the repo, and the goal.
 *
 * The refusals this tab is built around are all *before* anything is signed,
 * and all say what to do: a crew whose seats nobody on this computer fills, a
 * seat whose model the selected provider cannot actually run, a verifier
 * sharing a model vendor with a builder, and a seat whose vendor cannot be
 * established at all. The launch button stays disabled and the reason is on
 * screen — never a launch that half-happens and explains itself afterwards.
 */
export function NewCodingSessionCrewTab({
  channelId,
  disabled,
  onLaunched,
  providerAuthorityPubkey,
  providerInstanceRef,
  providerLabel,
  providerAllowedModels,
  model,
}: {
  channelId: string | null;
  disabled: boolean;
  onLaunched: (input: { channelId: string }) => void;
  providerAuthorityPubkey: string | null;
  providerInstanceRef: string | null;
  /** Name of the runtime every seat will run on, for the disclosure line. */
  providerLabel: string | null;
  /**
   * Models the selected provider runtime actually publishes.
   *
   * The vendor rule is decided on a model, and a model this runtime does not
   * have is replaced by its default — so without this list the check reads a
   * string nothing verified. An empty list is a refusal, not a pass.
   */
  providerAllowedModels: readonly string[];
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
  const provider = React.useMemo(
    () => ({ allowedModels: providerAllowedModels, label: providerLabel }),
    [providerAllowedModels, providerLabel],
  );
  // Same order as the launch: a model this provider cannot run is checked
  // before the vendor rule that would otherwise read it.
  const runnable = seats
    ? checkCodingSessionCrewSeatModels(seats, provider)
    : null;
  const family = seats ? checkCodingSessionCrewFamilies(seats) : null;
  // Only about the crew that is actually selected: with no crew to launch,
  // a missing provider is not yet anybody's problem to read.
  const refusal =
    selectedTeam === null
      ? null
      : ((providerInstanceRef === null || providerAuthorityPubkey === null
          ? "No coding-session provider is available on this computer, so there is nothing to run the crew on."
          : null) ??
        resolution?.error ??
        (runnable && !runnable.ok ? runnable.reason : null) ??
        (family && !family.ok ? family.reason : null));

  const { isLaunching, launch, result, steps } = useCodingSessionCrewLaunch({
    providerInstanceRef,
    providerAuthorityPubkey,
    workdir: workdir.trim().length > 0 ? workdir.trim() : null,
    title: title.trim().length > 0 ? title.trim() : null,
  });

  const canLaunch =
    !disabled &&
    !isLaunching &&
    channelId !== null &&
    selectedTeam !== null &&
    seats !== null &&
    refusal === null &&
    goal.trim().length > 0;

  const handleLaunch = React.useCallback(() => {
    if (!canLaunch || !channelId || !selectedTeam || !seats) return;
    setLaunchError(null);
    void (async () => {
      try {
        const result = await launch({
          channelId,
          goal,
          seats,
          primaryPersonaId: selectedTeam.crew.primary,
          provider,
        });
        if (result.ok) onLaunched({ channelId });
        else setLaunchError(result.failureReason);
      } catch (error) {
        setLaunchError(
          error instanceof Error
            ? error.message
            : "The crew could not be launched.",
        );
      }
    })();
  }, [
    canLaunch,
    channelId,
    goal,
    launch,
    onLaunched,
    provider,
    seats,
    selectedTeam,
  ]);

  return (
    <div className="flex flex-col gap-5" data-testid="new-coding-session-crew">
      <div className="flex flex-col gap-2">
        <label
          className="text-xs font-medium text-muted-foreground"
          htmlFor="coding-session-crew-team"
        >
          Crew
        </label>
        <select
          className="h-9 rounded-md border border-input bg-transparent px-3 text-sm disabled:opacity-50"
          data-testid="new-coding-session-crew-team"
          disabled={disabled || isLaunching || crewTeams.length === 0}
          id="coding-session-crew-team"
          onChange={(event) => setTeamId(event.target.value)}
          value={selectedTeam?.id ?? ""}
        >
          {crewTeams.length === 0 ? (
            <option value="">No crews on this computer</option>
          ) : null}
          {crewTeams.map((team) => (
            <option key={team.id} value={team.id}>
              {team.name}
            </option>
          ))}
        </select>
        <p className="text-2xs text-muted-foreground">
          {crewTeams.length === 0
            ? "A crew is a team whose seats carry roles. None of this computer's teams do yet."
            : `Every seat runs on ${providerLabel ?? "this computer's provider"}, ` +
              `in the directory below${
                model ? `, on ${model} unless its seat names its own` : ""
              }. Each seat's model is in the roster.`}
        </p>
      </div>

      {selectedTeam ? (
        <CodingSessionCrewRoster
          primaryPersonaId={selectedTeam.crew.primary}
          seats={seats}
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
          disabled={disabled || isLaunching}
          id="coding-session-crew-goal"
          onChange={(event) => setGoal(event.target.value)}
          placeholder="What is this crew for?"
          value={goal}
        />
        <p className="text-2xs text-muted-foreground">
          The lead's first turn carries this goal and the roster above.
        </p>
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
          disabled={disabled || isLaunching}
          id="coding-session-crew-title"
          onChange={(event) => setTitle(event.target.value)}
          placeholder="What is this session for?"
          value={title}
        />
      </div>

      <NewCodingSessionWorkdirField
        channelId={channelId}
        disabled={disabled || isLaunching}
        fallbackPath={null}
        onChange={setWorkdir}
        projectKey={null}
        value={workdir}
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
        A crew launch is not resumable. If it stops partway, the seats already
        created stay — finish the rest from the session itself.
      </p>

      <div className="flex shrink-0 items-center justify-end">
        <Button
          data-testid="new-coding-session-crew-launch"
          disabled={!canLaunch}
          onClick={handleLaunch}
          type="button"
        >
          {isLaunching ? (
            <LoaderCircle className="animate-spin motion-reduce:animate-none" />
          ) : (
            <Users />
          )}
          Launch crew
        </Button>
      </div>
    </div>
  );
}

/** The seats, each with the vendor the family rule will read. */
export function CodingSessionCrewRoster({
  primaryPersonaId,
  seats,
}: {
  primaryPersonaId: string;
  seats: ResolvedCodingSessionCrewSeat[] | null;
}) {
  if (!seats) return null;
  return (
    <ul
      className="flex flex-col gap-1 rounded-lg border border-border/60 bg-muted/30 px-3 py-2.5"
      data-testid="new-coding-session-crew-roster"
    >
      {seats.map((seat) => {
        const vendor = resolveCodingSessionSeatVendor(seat);
        return (
          <li
            className="flex items-baseline gap-2 text-sm"
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
            {seat.personaId === primaryPersonaId ? (
              <span className="text-2xs text-muted-foreground">
                · first turn
              </span>
            ) : null}
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
 * A crew tab that lists six roles and then says nothing implies six seats
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
