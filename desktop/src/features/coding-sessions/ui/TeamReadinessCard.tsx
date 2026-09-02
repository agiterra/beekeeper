import {
  CircleAlert,
  CircleCheck,
  CircleDashed,
  LoaderCircle,
  ShieldQuestion,
  TriangleAlert,
} from "lucide-react";

import type { ProjectRolePacksScan } from "@/shared/api/tauriTeams";
import type { NewCodingSessionTarget } from "../lib/newCodingSessionModel";
import type {
  TeamReadinessFact,
  TeamReadinessResponse,
  TeamReadinessState,
} from "@/shared/api/tauriTeamReadiness";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import {
  groupTeamReadinessFacts,
  teamReadinessLaunchGate,
  teamReadinessPrepareScope,
} from "../lib/teamReadinessModel";
import type { TeamReadinessPrepareStep } from "../lib/teamReadinessPrepare";

const SECTION_LABELS: Record<Exclude<TeamReadinessState, "ready">, string> = {
  blocked: "Blocked",
  unknown: "Unknown",
  awaiting_first_session: "Awaiting first session",
  limited: "Limited",
};

function sourceValue(value: string | boolean | null | undefined): string {
  if (value === null || value === undefined) return "unknown";
  return typeof value === "boolean" ? (value ? "yes" : "no") : value;
}

export function TeamReadinessCard(props: {
  readiness: TeamReadinessResponse | null;
  loading: boolean;
  readError: string | null;
  selectedRoles: readonly string[];
  scan: ProjectRolePacksScan | null;
  names: Readonly<Record<string, string>>;
  onNameChange: (role: string, name: string) => void;
  onBeginPrepare: () => void;
  onConfirmPrepare: () => void;
  onCancelPrepare: () => void;
  scanning: boolean;
  preparing: boolean;
  prepareSteps: readonly TeamReadinessPrepareStep[];
  prepareError: string | null;
  prepareWarning: string | null;
  externalBusy: boolean;
  runtimeTarget: NewCodingSessionTarget | null;
}) {
  const groups = groupTeamReadinessFacts(props.readiness?.facts ?? []);
  const firstSessionGate = teamReadinessLaunchGate({
    projectRef: props.readiness?.projectRef ?? "unchecked-project",
    loading: props.loading,
    error: props.readError,
    readiness: props.readiness,
    runtimeTarget: props.runtimeTarget,
  });
  const firstReady = firstSessionGate.allowed;
  const fullReady =
    firstReady &&
    props.readiness?.status === "ready" &&
    props.readiness.ready === true;
  const firstState =
    props.loading || props.readError || props.readiness?.status === "unknown"
      ? "unknown"
      : firstReady
        ? "ready"
        : "blocked";
  const allPacks = props.scan?.packs ?? [];
  // Finding 15: the launch's own seats are the question; every other pack is
  // refreshed and disclosed in one line, not turned into a field.
  const prepareScope = teamReadinessPrepareScope({
    packs: allPacks,
    selectedRoles: props.selectedRoles,
  });
  const missingRoles = prepareScope.missingRoles;

  return (
    <section
      aria-labelledby="team-readiness-title"
      className="flex flex-col gap-3 rounded-lg border border-border/60 bg-muted/20 p-3"
      data-testid="team-readiness-card"
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="flex min-w-0 items-start gap-2">
          {fullReady ? (
            <CircleCheck className="mt-0.5 size-4 shrink-0 text-emerald-600 dark:text-emerald-400" />
          ) : firstReady ? (
            <CircleDashed className="mt-0.5 size-4 shrink-0 text-sky-600 dark:text-sky-400" />
          ) : firstState === "unknown" ? (
            <ShieldQuestion className="mt-0.5 size-4 shrink-0 text-amber-600 dark:text-amber-400" />
          ) : (
            <CircleAlert className="mt-0.5 size-4 shrink-0 text-destructive" />
          )}
          <div className="min-w-0">
            <h3 className="text-sm font-medium" id="team-readiness-title">
              Team Readiness
            </h3>
            <p className="text-2xs text-muted-foreground">
              {props.loading
                ? "Checking this project…"
                : props.readError
                  ? `Readiness unknown: ${props.readError}`
                  : fullReady
                    ? "Ready for team sessions."
                    : firstReady
                      ? "Prepared for the first session. Full readiness awaits signed session facts."
                      : "Not prepared for the first team session."}
            </p>
          </div>
        </div>
        <section
          aria-label="Readiness summary"
          className="flex flex-wrap gap-1 text-2xs"
        >
          <span
            className={cn(
              "rounded-full border px-2 py-0.5",
              firstState === "ready"
                ? "border-emerald-500/40 text-emerald-700 dark:text-emerald-300"
                : firstState === "unknown"
                  ? "border-amber-500/40 text-amber-700 dark:text-amber-300"
                  : "border-destructive/40 text-destructive",
            )}
          >
            First session: {firstState}
          </span>
          <span className="rounded-full border border-border px-2 py-0.5 text-muted-foreground">
            Full: {fullReady ? "ready" : "not ready"}
          </span>
        </section>
      </div>

      {props.readiness ? (
        <dl className="grid gap-x-3 gap-y-1 text-2xs text-muted-foreground sm:grid-cols-[auto_minmax(0,1fr)]">
          <dt>Checkout</dt>
          <dd className="break-all">
            {sourceValue(props.readiness.source.checkoutPath)}
          </dd>
          <dt>Checkout HEAD</dt>
          <dd className="break-all">
            {sourceValue(props.readiness.source.checkoutCommit)} · dirty:{" "}
            {sourceValue(props.readiness.source.checkoutDirty)}
          </dd>
          <dt>App source</dt>
          <dd className="break-all">
            {sourceValue(props.readiness.source.appCommit)} · dirty:{" "}
            {sourceValue(props.readiness.source.appSourceDirty)}
          </dd>
        </dl>
      ) : null}

      {(
        ["blocked", "unknown", "awaiting_first_session", "limited"] as const
      ).map((state) =>
        groups[state].length > 0 ? (
          <FactSection facts={groups[state]} key={state} state={state} />
        ) : null,
      )}

      {props.scan ? (
        <fieldset
          className="flex flex-col gap-3 rounded-md border border-border bg-background/60 p-3"
          data-testid="team-readiness-role-confirmation"
        >
          <legend className="sr-only">Confirm the names for this launch</legend>
          <div>
            <p className="text-sm font-medium">
              Confirm the names for this launch
            </p>
            <p className="text-2xs text-muted-foreground">
              Launch roles: {props.selectedRoles.join(", ") || "none"}, from{" "}
              {props.scan.directory}.
            </p>
            {prepareScope.otherLine ? (
              <p
                className="text-2xs text-muted-foreground"
                data-testid="team-readiness-other-packs"
              >
                {prepareScope.otherLine}
              </p>
            ) : null}
          </div>
          {missingRoles.length > 0 ? (
            <p className="text-sm text-destructive" role="alert">
              Missing selected role{" "}
              {missingRoles.length === 1 ? "pack" : "packs"}:{" "}
              {missingRoles.join(", ")}. Add{" "}
              {missingRoles.length === 1 ? "it" : "them"} to this project's
              role-pack folder, then scan again.
            </p>
          ) : null}
          {prepareScope.confirm.length > 0 ? (
            <div className="grid gap-2 sm:grid-cols-2">
              {prepareScope.confirm.map((pack) => (
                <label
                  className="flex flex-col gap-1 text-xs"
                  htmlFor={`team-readiness-name-${pack.role}`}
                  key={pack.role}
                >
                  <span className="font-medium capitalize">
                    {pack.role}
                    <span className="ml-1 font-normal text-muted-foreground">
                      · selected launch role
                    </span>
                  </span>
                  <Input
                    aria-label={`${pack.role} agent name`}
                    disabled={props.preparing || props.externalBusy}
                    id={`team-readiness-name-${pack.role}`}
                    onChange={(event) =>
                      props.onNameChange(pack.role, event.target.value)
                    }
                    value={props.names[pack.role] ?? ""}
                  />
                </label>
              ))}
            </div>
          ) : (
            <p className="text-sm text-destructive" role="alert">
              {allPacks.length === 0
                ? "No role packs were discovered. Add role packs to this project, then scan again before preparing."
                : "This launch names no role whose pack is in this folder. Pick the launch's roles, then scan again before preparing."}
            </p>
          )}
          <p className="text-2xs text-muted-foreground">
            Beekeeper may ask macOS to unlock signing keys. This screen never
            receives or stores that password.
          </p>
          <div className="flex flex-wrap justify-end gap-2">
            <Button
              disabled={props.preparing || props.externalBusy}
              onClick={props.onCancelPrepare}
              type="button"
              variant="ghost"
            >
              Cancel
            </Button>
            <Button
              data-testid="team-readiness-prepare-confirm"
              disabled={
                props.preparing ||
                props.externalBusy ||
                missingRoles.length > 0 ||
                prepareScope.confirm.length === 0
              }
              onClick={props.onConfirmPrepare}
              type="button"
            >
              {props.preparing ? (
                <LoaderCircle className="animate-spin motion-reduce:animate-none" />
              ) : null}
              Confirm and prepare
            </Button>
          </div>
        </fieldset>
      ) : (
        <Button
          className="self-start"
          data-testid="team-readiness-prepare"
          disabled={
            props.loading ||
            props.scanning ||
            props.preparing ||
            props.externalBusy
          }
          onClick={props.onBeginPrepare}
          type="button"
          variant="outline"
        >
          {props.scanning ? (
            <LoaderCircle className="animate-spin motion-reduce:animate-none" />
          ) : null}
          Prepare this project for teams
        </Button>
      )}

      {props.prepareSteps.length > 0 ? (
        <ol
          className="flex flex-col gap-1"
          data-testid="team-readiness-prepare-steps"
        >
          {props.prepareSteps.map((step) => (
            <li
              className={cn(
                "flex items-start gap-2 text-2xs",
                step.state === "failed"
                  ? "text-destructive"
                  : "text-muted-foreground",
              )}
              key={step.id}
            >
              <span aria-hidden="true">
                {step.state === "done"
                  ? "✓"
                  : step.state === "failed"
                    ? "×"
                    : step.state === "running"
                      ? "…"
                      : "○"}
              </span>
              <span>
                {step.label}: {step.state}
                {step.detail ? ` — ${step.detail}` : ""}
              </span>
            </li>
          ))}
        </ol>
      ) : null}

      {props.prepareError ? (
        <p
          className="flex items-start gap-2 text-sm text-destructive"
          role="alert"
        >
          <TriangleAlert className="mt-0.5 size-4 shrink-0" />
          {props.prepareError}
        </p>
      ) : null}
      {props.prepareWarning ? (
        <p
          className="flex items-start gap-2 text-sm text-amber-700 dark:text-amber-300"
          role="status"
        >
          <TriangleAlert className="mt-0.5 size-4 shrink-0" />
          {props.prepareWarning}
        </p>
      ) : null}
    </section>
  );
}

function FactSection({
  facts,
  state,
}: {
  facts: readonly TeamReadinessFact[];
  state: Exclude<TeamReadinessState, "ready">;
}) {
  return (
    <section aria-label={SECTION_LABELS[state]} data-readiness-state={state}>
      <h4 className="text-xs font-medium">{SECTION_LABELS[state]}</h4>
      <ul className="mt-1 flex flex-col gap-1">
        {facts.map((fact) => (
          <li
            className="text-2xs text-muted-foreground"
            key={`${fact.scope}:${fact.code}`}
          >
            <span className="font-medium text-foreground">{fact.code}</span> ·{" "}
            {fact.summary}
            {fact.remedy ? ` Remedy: ${fact.remedy}` : ""}
          </li>
        ))}
      </ul>
    </section>
  );
}
