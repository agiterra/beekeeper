import * as React from "react";
import { Button } from "@/shared/ui/button";
import type { ProjectTeamSetupActivation } from "../lib/projectTeamSetup";
import {
  projectRosterReadiness,
  type ProjectRosterReadiness,
} from "../lib/projectRosterReadiness";
import {
  projectTeamSetupStage,
  type ProjectTeamSetupStage,
} from "../lib/projectTeamSetupStage";
import {
  agentName,
  shortPubkey,
  useProjectTeamSetupAgents,
} from "./ProjectTeamSetupAgents";
import {
  InstalledRoleRow,
  NAME_UNAVAILABLE,
} from "./ProjectTeamSetupInstalledRoles";

/**
 * Whether the installed agents are this project's agents on this computer,
 * read from the setup's agent directory. `null` before anything is installed.
 */
export function useProjectTeamSetupRoster(
  projectRef: string,
  activation: ProjectTeamSetupActivation | null | undefined,
): ProjectRosterReadiness | null {
  const { agents } = useProjectTeamSetupAgents();
  const installedRoles = activation?.installation.installedRoles;
  const leadPubkey = activation?.lead.leadPubkey ?? null;
  return React.useMemo(
    () =>
      installedRoles
        ? projectRosterReadiness({
            projectRef,
            installedRoles,
            agents,
            leadPubkey,
          })
        : null,
    [agents, installedRoles, leadPubkey, projectRef],
  );
}

/** Render `code spans` in a stage sentence as code; the rest stays text. */
export function ProjectTeamSetupSentence({ text }: { text: string }) {
  const parts = text.split("`");
  return (
    <>
      {parts.map((part, index) =>
        index % 2 === 1 ? (
          <code
            className="rounded bg-muted px-1 font-mono text-xs"
            // biome-ignore lint/suspicious/noArrayIndexKey: parts of one fixed sentence, never reordered.
            key={index}
          >
            {part}
          </code>
        ) : (
          // biome-ignore lint/suspicious/noArrayIndexKey: parts of one fixed sentence, never reordered.
          <React.Fragment key={index}>{part}</React.Fragment>
        ),
      )}
    </>
  );
}

export type ProjectTeamSetupRosterActions = {
  busy: boolean;
  creatingChannel: boolean;
  onRetryInstall: () => void;
  onStartLead: () => void;
  onRetryChannel: () => void;
};

type RosterAction = { label: string; busyLabel: string; run: () => void };

function rosterAction(
  stage: ProjectTeamSetupStage,
  activation: ProjectTeamSetupActivation,
  roster: ProjectRosterReadiness,
  actions: ProjectTeamSetupRosterActions,
  directory: ReturnType<typeof useProjectTeamSetupAgents>,
): RosterAction | null {
  const { lead } = activation;
  switch (stage.id) {
    case "roster_blocked":
      // Reinstalling never moves another project's agent here, so offering it
      // would loop on a remedy that cannot work; the Agents tab link beside
      // the action is the way forward.
      if (roster.entries.some((entry) => entry.association === "other-project"))
        return null;
      return {
        label: "Retry installation",
        busyLabel: "Installing…",
        run: actions.onRetryInstall,
      };
    case "roster_uncertain": {
      if (roster.status === "empty")
        return {
          label: "Retry installation",
          busyLabel: "Installing…",
          run: actions.onRetryInstall,
        };
      const refresh = directory.refreshAgents;
      return refresh
        ? { label: "Check again", busyLabel: "Checking…", run: refresh }
        : null;
    }
    case "start_lead":
      return {
        label: "Start the project lead",
        busyLabel: "Starting lead…",
        run: actions.onStartLead,
      };
    case "lead_uncertain":
      if (lead.status !== "unknown") return null;
      return lead.sessionRef
        ? {
            label: "Retry lead handoff",
            busyLabel: "Starting lead…",
            run: actions.onStartLead,
          }
        : {
            label: "Retry session-channel setup",
            busyLabel: "Preparing project session channel…",
            run: actions.onRetryChannel,
          };
    case "lead_started": {
      const open = directory.openLeadSession;
      const { channelId, sessionRef } = lead;
      return open && channelId && sessionRef
        ? {
            label: "Open session",
            busyLabel: "Opening…",
            run: () => open({ channelId, sessionRef }),
          }
        : null;
    }
    default:
      return null;
  }
}

/**
 * The end of setup: who this project's agents are on this computer, whether
 * the lead can hire each of them, and the one next action. It never reads as
 * complete while an installed role's agent lacks the project association the
 * lead's hires require. Pubkeys and session identifiers stay under details.
 */
export function ProjectTeamSetupRoster({
  projectRef,
  projectName,
  activation,
  actions,
}: {
  projectRef: string;
  projectName?: string;
  activation: ProjectTeamSetupActivation;
  actions: ProjectTeamSetupRosterActions;
}) {
  const directory = useProjectTeamSetupAgents();
  const roster = useProjectTeamSetupRoster(projectRef, activation);
  if (!roster) return null;
  // The same derivation the setup header uses, so the two never disagree.
  const stage = projectTeamSetupStage({
    snapshot: "saved",
    publication: { status: "adopted" },
    activation,
    roster,
    projectName,
  });
  const action = rosterAction(stage, activation, roster, actions, directory);
  const pending = actions.busy || actions.creatingChannel;
  const { lead } = activation;
  return (
    <section
      className="min-w-0 space-y-2 rounded-md border p-3"
      data-roster={roster.status}
      data-testid="project-team-setup-roster"
    >
      <h4 className="break-words text-sm font-medium">
        {projectName ? `${projectName} agents` : "Project agents"}
      </h4>
      {roster.entries.length === 0 ? (
        <p className="text-sm text-muted-foreground">
          The installation recorded no agents.
        </p>
      ) : (
        <ul className="space-y-1">
          {roster.entries.map((entry) => (
            <InstalledRoleRow entry={entry} key={entry.agentPubkey} />
          ))}
        </ul>
      )}
      {lead.leadPubkey ? null : (
        <p className="text-sm text-muted-foreground">
          No lead identity is recorded for this installation.
        </p>
      )}
      <p
        className="break-words text-sm"
        data-stage={stage.id}
        data-state={stage.state}
        data-testid="project-team-setup-roster-next"
        role={stage.state === "blocked" ? "alert" : "status"}
      >
        <ProjectTeamSetupSentence text={stage.next} />
      </p>
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        {action ? (
          <Button
            className="max-w-full whitespace-normal"
            disabled={pending}
            onClick={action.run}
            type="button"
          >
            {pending ? action.busyLabel : action.label}
          </Button>
        ) : null}
        {directory.openAgentsTab ? (
          <Button
            className="max-w-full whitespace-normal"
            onClick={directory.openAgentsTab}
            type="button"
            variant="link"
          >
            Open the project's Agents tab
          </Button>
        ) : null}
      </div>
      <details className="text-sm text-muted-foreground">
        <summary className="cursor-pointer">Identity details</summary>
        <ul className="mt-1 space-y-0.5">
          {roster.entries.map((entry) => (
            <li className="break-all" key={entry.agentPubkey}>
              {agentName(directory.names, entry.agentPubkey) ??
                entry.name ??
                NAME_UNAVAILABLE}{" "}
              · {entry.role} ·{" "}
              <span className="font-mono text-xs" title={entry.agentPubkey}>
                {shortPubkey(entry.agentPubkey)}
              </span>
            </li>
          ))}
        </ul>
        {lead.channelId || lead.sessionRef ? (
          <div data-testid="project-team-setup-lead-session-details">
            {lead.channelId ? (
              <p className="break-all">Channel {lead.channelId}</p>
            ) : null}
            {lead.sessionRef ? (
              <p className="break-all">Session {lead.sessionRef}</p>
            ) : null}
          </div>
        ) : null}
      </details>
    </section>
  );
}
