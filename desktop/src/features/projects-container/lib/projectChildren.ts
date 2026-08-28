import type { RemoteTerminal } from "@/features/builtin-shell/observe/useProjectTerminals";
import type { Channel } from "@/shared/api/types";
import type { ShellSessionInfo } from "@/shared/api/tauriShell";
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

/**
 * One row in a project's sidebar child list; the type picks the icon. The
 * sidebar lists only channels and interactive work (coding sessions and
 * terminals) — repositories, workflows, agents and Pulse live on the project
 * page, not here.
 */
export type ProjectChildRow =
  | { type: "coding-session"; entry: ProjectCodingSessionShelfEntry }
  | { type: "channel"; channel: Channel }
  | { type: "forum"; channel: Channel }
  | { type: "shell"; session: ShellSessionInfo }
  | { type: "remote-shell"; terminal: RemoteTerminal };

/** Fixed display order of the flat list — live coding sessions on top: a
 * session is the only child that changes while you watch it. */
export const PROJECT_CHILD_TYPE_RANK: Record<ProjectChildRow["type"], number> =
  {
    "coding-session": 0,
    channel: 1,
    forum: 2,
    shell: 3,
    "remote-shell": 4,
  };

/** Stable, cross-type-unique React key for a child row. */
export function projectChildKey(row: ProjectChildRow): string {
  switch (row.type) {
    case "coding-session":
      return `session:${row.entry.channelId}:${row.entry.generationId}`;
    case "channel":
      return `channel:${row.channel.id}`;
    case "forum":
      return `forum:${row.channel.id}`;
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
    case "channel":
    case "forum":
      return row.channel.name;
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
  streamChannels: Channel[];
  forumChannels: Channel[];
  shellSessions: ShellSessionInfo[];
  remoteTerminals?: RemoteTerminal[];
}): ProjectChildRow[] {
  const rows: ProjectChildRow[] = [
    ...(input.codingSessions ?? []).map(
      (entry): ProjectChildRow => ({ type: "coding-session", entry }),
    ),
    ...input.streamChannels.map(
      (channel): ProjectChildRow => ({ type: "channel", channel }),
    ),
    ...input.forumChannels.map(
      (channel): ProjectChildRow => ({ type: "forum", channel }),
    ),
    ...input.shellSessions.map(
      (session): ProjectChildRow => ({ type: "shell", session }),
    ),
    ...(input.remoteTerminals ?? []).map(
      (terminal): ProjectChildRow => ({ type: "remote-shell", terminal }),
    ),
  ];
  return rows.sort(compareProjectChildren);
}
