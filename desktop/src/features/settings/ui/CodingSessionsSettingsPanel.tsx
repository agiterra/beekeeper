import * as React from "react";

import { AgentHostAutostartCard } from "@/features/coding-sessions/ui/AgentHostAutostartCard";
import { CodingSessionCapacityCard } from "@/features/coding-sessions/ui/CodingSessionCapacityCard";
import { CodingSessionNamingCard } from "@/features/coding-sessions/ui/CodingSessionNamingCard";
import {
  CODING_SESSION_HIRE_MAX_SEATS_CEILING,
  DEFAULT_CODING_SESSION_HIRE_POLICY,
  codingSessionHireAllowedRoles,
  parseCodingSessionHireMaxSeatsInput,
  readCodingSessionHirePolicy,
  writeCodingSessionHirePolicy,
  type CodingSessionHirePolicy,
} from "@/features/coding-sessions/lib/codingSessionHirePolicy";
import { useManagedAgentsQuery } from "@/features/agents/hooks";
import { getCodingSessionProviderRuntimes } from "@/shared/api/tauriSessionProvider";
import { useQuery } from "@tanstack/react-query";
import { Input } from "@/shared/ui/input";
import { Switch } from "@/shared/ui/switch";

import { DefaultRepositoryFolderCard } from "./DefaultRepositoryFolderCard";
import {
  SettingsOptionGroup,
  SettingsOptionGroupList,
} from "./SettingsOptionGroup";

/** Settings for the coding sessions this computer runs. */
export function CodingSessionsSettingsPanel() {
  return (
    <SettingsOptionGroupList>
      <SettingsOptionGroup
        data-testid="settings-agent-host"
        description="Your coding sessions run in a background agent host, not in this window. With it installed they keep going when you quit Beekeeper and start again when you log in; without it they end with the app. A menu bar icon shows what is running either way."
        title="Running when Beekeeper is closed"
      >
        <AgentHostAutostartCard />
      </SettingsOptionGroup>
      <SettingsOptionGroup
        data-testid="settings-coding-sessions"
        description="How many coding sessions this computer will run at once, how long one turn may go silent, and how many turns a team session may take before its agents are refused. Each live session is an agent process here — these limits are Beekeeper's own, not your model provider's."
        title="Sessions"
      >
        <CodingSessionCapacityCard />
      </SettingsOptionGroup>
      <SettingsOptionGroup
        data-testid="settings-coding-session-hiring"
        description="A lead running in one of your sessions can ask this computer to seat another agent — its own worktree, its own process, on your machine. These are the bounds it may do that within."
        title="Hiring"
      >
        <CodingSessionHiringCard />
      </SettingsOptionGroup>
      <SettingsOptionGroup
        data-testid="settings-project-repositories"
        description="Where this computer clones a new project's code repository. Creating a project and Finish repository setup pre-fill their folder row from here; the same value is Edit community → Repositories folder."
        title="Project repositories"
      >
        <DefaultRepositoryFolderCard />
      </SettingsOptionGroup>
      <SettingsOptionGroup
        data-testid="settings-coding-session-naming"
        description="Name a new session from its first message. Off unless you choose a model — the message leaves this computer only for the endpoint you name."
        title="Session names"
      >
        <CodingSessionNamingCard />
      </SettingsOptionGroup>
    </SettingsOptionGroupList>
  );
}

/**
 * The standing hiring policy, as a form.
 *
 * Everything a hire spends is this computer's — a process, a worktree, a key
 * staged from the keyring — and the request comes from an agent, not a person.
 * So the policy is set here, once, in advance, and every hire is answered
 * against it without a prompt. Each control says what it actually enforces:
 * the roles line names the roles this machine can honestly offer (the ones
 * whose packs are installed), not an aspiration.
 */
export function CodingSessionHiringCard() {
  const [policy, setPolicy] = React.useState<CodingSessionHirePolicy>(() =>
    readCodingSessionHirePolicy(),
  );
  const [seatsDraft, setSeatsDraft] = React.useState<string>(() =>
    String(readCodingSessionHirePolicy().maxSeatsPerUmbrella),
  );
  const managedAgents = useManagedAgentsQuery();
  const candidates = React.useMemo(
    () =>
      (managedAgents.data ?? []).map((agent) => ({
        pubkey: agent.pubkey,
        name: agent.name,
        homeRole: agent.homeRole,
        ...(agent.hasRolePack === undefined
          ? {}
          : { hasRolePack: agent.hasRolePack }),
      })),
    [managedAgents.data],
  );
  const installedRoles = codingSessionHireAllowedRoles(
    DEFAULT_CODING_SESSION_HIRE_POLICY,
    candidates,
  );
  const allowedRoles = policy.allowedRoles;
  const runtimesQuery = useQuery({
    queryKey: ["coding-session-provider-runtimes"],
    queryFn: getCodingSessionProviderRuntimes,
  });
  // Only runtimes a hire could actually land on: a signed-out or uninstalled
  // one in this list would be a control over something that cannot run.
  const runtimes = (runtimesQuery.data ?? []).filter(
    (runtime) => runtime.authState === "ready",
  );
  const allowedProviders = policy.allowedProviderInstanceRefs;

  const commit = (next: CodingSessionHirePolicy) => {
    setPolicy(next);
    writeCodingSessionHirePolicy(next);
  };

  return (
    <div
      className="flex flex-col gap-4 px-4 py-4"
      data-testid="settings-coding-session-hire-policy"
    >
      <div className="flex items-start justify-between gap-4">
        <div className="flex flex-col gap-1">
          <p className="text-sm font-medium">Let leads hire</p>
          <p className="text-xs text-muted-foreground">
            {policy.enabled
              ? "A lead may ask for a seat. Every request is answered against the bounds below — you are not asked at the time."
              : "Every hire request is refused HIRE_OFF, and the lead is told so. Nothing is seated."}
          </p>
        </div>
        <Switch
          checked={policy.enabled}
          data-testid="coding-session-hire-enabled"
          onCheckedChange={(checked) =>
            commit({ ...policy, enabled: checked === true })
          }
        />
      </div>

      <div className="flex flex-col gap-1.5">
        <p className="text-xs font-medium text-muted-foreground">
          Roles a lead may hire
        </p>
        <div className="flex flex-wrap gap-3">
          {installedRoles.length === 0 ? (
            <p
              className="text-xs text-muted-foreground"
              data-testid="coding-session-hire-no-roles"
            >
              No role packs are installed on this computer, so there is no role
              a lead could be given. Install team roles on the Agents screen.
            </p>
          ) : (
            installedRoles.map((role) => {
              const checked =
                allowedRoles === null || allowedRoles.includes(role);
              return (
                <label className="flex items-center gap-1.5 text-xs" key={role}>
                  <input
                    checked={checked}
                    data-testid={`coding-session-hire-role-${role}`}
                    onChange={(event) => {
                      const next = new Set(allowedRoles ?? installedRoles);
                      if (event.target.checked) next.add(role);
                      else next.delete(role);
                      commit({ ...policy, allowedRoles: [...next].sort() });
                    }}
                    type="checkbox"
                  />
                  {role}
                </label>
              );
            })
          )}
        </div>
        <p className="text-2xs text-muted-foreground">
          {allowedRoles === null
            ? "Every role whose pack is installed here. A role with no pack is not offered — this computer would have nothing to give the seat."
            : allowedRoles.length === 0
              ? "No roles: every hire is refused HIRE_ROLE_NOT_ALLOWED."
              : `Only ${allowedRoles.join(", ")}. Anything else is refused HIRE_ROLE_NOT_ALLOWED.`}
        </p>
      </div>

      <div className="flex flex-col gap-1.5">
        <label
          className="text-xs font-medium text-muted-foreground"
          htmlFor="coding-session-hire-max-seats"
        >
          Live seats per session
        </label>
        <Input
          className="h-9 w-24"
          data-testid="coding-session-hire-max-seats"
          id="coding-session-hire-max-seats"
          inputMode="numeric"
          max={CODING_SESSION_HIRE_MAX_SEATS_CEILING}
          min={1}
          onBlur={() => setSeatsDraft(String(policy.maxSeatsPerUmbrella))}
          onChange={(event) => {
            setSeatsDraft(event.target.value);
            commit({
              ...policy,
              maxSeatsPerUmbrella: parseCodingSessionHireMaxSeatsInput(
                event.target.value,
                policy.maxSeatsPerUmbrella,
              ),
            });
          }}
          type="number"
          value={seatsDraft}
        />
        <p className="text-2xs text-muted-foreground">
          Counted across every seat already running in one session, whoever
          started it. A hire past this is refused HIRE_LIMIT. This is separate
          from the session ceiling above, which counts every session on this
          computer.
        </p>
      </div>

      <div className="flex flex-col gap-1.5">
        <p className="text-xs font-medium text-muted-foreground">
          Providers a hire may run on
        </p>
        <div className="flex flex-wrap gap-3">
          {runtimes.length === 0 ? (
            <p
              className="text-xs text-muted-foreground"
              data-testid="coding-session-hire-no-providers"
            >
              No runtime on this computer is signed in and ready, so a hire has
              nothing to run on and is refused HIRE_PROVIDER_NOT_ALLOWED.
            </p>
          ) : (
            runtimes.map((runtime) => {
              const checked =
                allowedProviders === null ||
                allowedProviders.includes(runtime.instanceRef);
              return (
                <label
                  className="flex items-center gap-1.5 text-xs"
                  key={runtime.instanceRef}
                >
                  <input
                    checked={checked}
                    data-testid={`coding-session-hire-provider-${runtime.instanceRef}`}
                    onChange={(event) => {
                      const next = new Set(
                        allowedProviders ??
                          runtimes.map((entry) => entry.instanceRef),
                      );
                      if (event.target.checked) next.add(runtime.instanceRef);
                      else next.delete(runtime.instanceRef);
                      commit({
                        ...policy,
                        allowedProviderInstanceRefs: [...next].sort(),
                      });
                    }}
                    type="checkbox"
                  />
                  {runtime.label}
                </label>
              );
            })
          )}
        </div>
        <p className="text-2xs text-muted-foreground">
          {allowedProviders === null
            ? "Every runtime this computer runs. A lead that names one of them gets it; a lead that names none gets the first."
            : allowedProviders.length === 0
              ? "No providers: every hire is refused HIRE_PROVIDER_NOT_ALLOWED."
              : `Only ${allowedProviders.join(", ")}. Anything else is refused HIRE_PROVIDER_NOT_ALLOWED.`}
        </p>
      </div>
    </div>
  );
}
