import type { RemoteTerminal } from "@/features/builtin-shell/observe/useProjectTerminals";
import type { Channel } from "@/shared/api/types";
import type { ShellSessionInfo } from "@/shared/api/tauriShell";
import type { Workflow } from "@/shared/api/workflowTypes";
import type { Repository as CodeRepo } from "@/features/projects/hooks";
import { KIND_MANAGED_AGENT, KIND_PERSONA } from "@/shared/constants/kinds";

import { parseMemberRef } from "./projectContainerModel";
import {
  compareProjectCodingSessionEntries,
  type ProjectCodingSessionShelfEntry,
} from "./projectCodingSessionShelf";
import type { ProjectContainer } from "../hooks";

export type ProjectAgentRow = {
  key: string;
  label: string;
};

/**
 * Resolves a project's curated agent refs against the locally-known personas
 * and managed agents; unresolved refs fall back to their dtag so a project
 * curated on another device still shows every member.
 */
export function projectAgentRows(
  project: ProjectContainer,
  personasById: ReadonlyMap<string, string>,
  managedAgentsByPubkey: ReadonlyMap<string, string>,
): ProjectAgentRow[] {
  const rows: ProjectAgentRow[] = [];
  for (const addr of project.agentAddrs) {
    const ref = parseMemberRef(addr);
    if (!ref) continue;
    let label: string | undefined;
    if (ref.kind === KIND_MANAGED_AGENT) {
      label = managedAgentsByPubkey.get(ref.dtag.toLowerCase());
    } else if (ref.kind === KIND_PERSONA) {
      label = personasById.get(ref.dtag);
    }
    rows.push({ key: addr, label: label ?? ref.dtag });
  }
  return rows;
}

/** One row in a project's flat child list; the type picks the icon. */
export type ProjectChildRow =
  | { type: "coding-session"; entry: ProjectCodingSessionShelfEntry }
  // A singleton row, not a collection: one Pulse per project, always present
  // when the caller opts in — its screen renders the confirmed-empty state
  // rather than the row disappearing when a project is quiet.
  | { type: "pulse" }
  | { type: "channel"; channel: Channel }
  | { type: "forum"; channel: Channel }
  | { type: "repo"; repo: CodeRepo }
  | { type: "workflow"; workflow: Workflow }
  | { type: "agent"; agent: ProjectAgentRow }
  | { type: "shell"; session: ShellSessionInfo }
  | { type: "remote-shell"; terminal: RemoteTerminal };

/** Fixed display order of the flat list — mirrors the old subsection order,
 * with forums promoted next to channels and live coding sessions on top:
 * a session is the only child that changes while you watch it. */
export const PROJECT_CHILD_TYPE_RANK: Record<ProjectChildRow["type"], number> =
  {
    "coding-session": 0,
    // Pulse sits directly below the sessions it describes: it is the
    // coordination answer for the same live work, not another collection.
    pulse: 1,
    channel: 2,
    forum: 3,
    repo: 4,
    workflow: 5,
    agent: 6,
    shell: 7,
    "remote-shell": 8,
  };

/** Stable, cross-type-unique React key for a child row. */
export function projectChildKey(row: ProjectChildRow): string {
  switch (row.type) {
    case "coding-session":
      return `session:${row.entry.channelId}:${row.entry.generationId}`;
    case "pulse":
      return "pulse";
    case "channel":
      return `channel:${row.channel.id}`;
    case "forum":
      return `forum:${row.channel.id}`;
    case "repo":
      return `repo:${row.repo.repoAddress}`;
    case "workflow":
      return `workflow:${row.workflow.id}`;
    case "agent":
      return `agent:${row.agent.key}`;
    case "shell":
      return `shell:${row.session.sessionId}`;
    case "remote-shell":
      return `remote-shell:${row.terminal.ownerPubkey}:${row.terminal.sessionId}`;
  }
}

export function projectChildLabel(row: ProjectChildRow): string {
  switch (row.type) {
    case "coding-session":
      return row.entry.label;
    case "pulse":
      // "Project Pulse", never bare "Pulse": the pinned top-level social
      // activity feed is also called Pulse, and with both preview flags on a
      // user would see the same word meaning two unrelated things in one
      // sidebar. Matches the preview feature's own display name.
      return "Project Pulse";
    case "channel":
    case "forum":
      return row.channel.name;
    case "repo":
      return row.repo.name;
    case "workflow":
      return row.workflow.name;
    case "agent":
      return row.agent.label;
    case "shell":
      return row.session.title;
    case "remote-shell":
      return row.terminal.title;
  }
}

export function compareProjectChildren(
  a: ProjectChildRow,
  b: ProjectChildRow,
): number {
  const byRank =
    PROJECT_CHILD_TYPE_RANK[a.type] - PROJECT_CHILD_TYPE_RANK[b.type];
  if (byRank !== 0) return byRank;
  // Sessions keep the shelf's activity order (working → unknown → idle, then
  // newest first). Alphabetizing them would sort by a label that is mostly the
  // same word plus a generation number, burying the one that is running now.
  if (a.type === "coding-session" && b.type === "coding-session") {
    return compareProjectCodingSessionEntries(a.entry, b.entry);
  }
  const byLabel = projectChildLabel(a).localeCompare(
    projectChildLabel(b),
    undefined,
    { sensitivity: "base" },
  );
  return byLabel !== 0
    ? byLabel
    : projectChildKey(a).localeCompare(projectChildKey(b));
}

/**
 * Merge every kind of project child into the single flat, type-ranked,
 * alphabetized list the sidebar renders. Callers apply feature gating and
 * visibility caps before calling.
 */
export function buildProjectChildren(input: {
  codingSessions?: ProjectCodingSessionShelfEntry[];
  /** Emit the singleton Pulse row. Callers pass the `project-pulse` preview
   * flag AND `!isFallback`: the local General placeholder has no project
   * coordinate, so its Pulse row would open a screen that can never load. */
  includePulse?: boolean;
  streamChannels: Channel[];
  forumChannels: Channel[];
  repos: CodeRepo[];
  workflows: Workflow[];
  agents: ProjectAgentRow[];
  shellSessions: ShellSessionInfo[];
  remoteTerminals?: RemoteTerminal[];
}): ProjectChildRow[] {
  const rows: ProjectChildRow[] = [
    ...(input.codingSessions ?? []).map(
      (entry): ProjectChildRow => ({ type: "coding-session", entry }),
    ),
    ...(input.includePulse ? [{ type: "pulse" } as ProjectChildRow] : []),
    ...input.streamChannels.map(
      (channel): ProjectChildRow => ({ type: "channel", channel }),
    ),
    ...input.forumChannels.map(
      (channel): ProjectChildRow => ({ type: "forum", channel }),
    ),
    ...input.repos.map((repo): ProjectChildRow => ({ type: "repo", repo })),
    ...input.workflows.map(
      (workflow): ProjectChildRow => ({ type: "workflow", workflow }),
    ),
    ...input.agents.map((agent): ProjectChildRow => ({ type: "agent", agent })),
    ...input.shellSessions.map(
      (session): ProjectChildRow => ({ type: "shell", session }),
    ),
    ...(input.remoteTerminals ?? []).map(
      (terminal): ProjectChildRow => ({ type: "remote-shell", terminal }),
    ),
  ];
  return rows.sort(compareProjectChildren);
}
