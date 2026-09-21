import type { TodoList } from "@/features/project-todos/lib/todoFold";
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
  /** The agent's role, when this computer knows it. */
  role: string | null;
  /**
   * `"published"` — a curated ref carried on the project head, readable by
   * anyone. `"local"` — a managed agent on THIS computer associated with
   * this project, which a private project never publishes.
   */
  source: "published" | "local";
};

/** The local evidence of project membership: one managed agent record. */
export type ProjectLocalAgent = {
  pubkey: string;
  name: string;
  homeRole?: string | null;
  projectRef?: string | null;
};

/**
 * Every agent this screen can honestly say belongs to the project: the
 * curated refs on the project head, plus this computer's own managed agents
 * associated with it.
 *
 * # The finding this exists for
 *
 * Ledger 207(3). On 2026-09-20 the Overview of "Kettle Smoke" printed
 * "Agents 0 — No agents in this project" directly under a Members panel
 * listing that project's eight agents as collaborators. This function read
 * `project.agentAddrs` alone — the *published* curation — and a private
 * project publishes none ("a private project publishes none", the founder
 * form's own words), so the count was structurally zero while the local
 * store held eight. Two panels on one screen contradicting each other is a
 * bug of the same severity as a crash.
 *
 * Local rows are matched on `projectRef`, the only local evidence of
 * membership (`shared/lib/projectAgentAssociation.ts`): a matching role, an
 * installed pack or a past seat is not membership and is not counted here.
 */
export function projectAgentRows(
  project: ProjectContainer,
  personasById: ReadonlyMap<string, string>,
  managedAgentsByPubkey: ReadonlyMap<string, string>,
  localAgents: readonly ProjectLocalAgent[] = [],
): ProjectAgentRow[] {
  const rows: ProjectAgentRow[] = [];
  const seen = new Set<string>();
  for (const addr of project.agentAddrs) {
    const ref = parseMemberRef(addr);
    if (!ref) continue;
    let label: string | undefined;
    if (ref.kind === KIND_MANAGED_AGENT) {
      label = managedAgentsByPubkey.get(ref.dtag.toLowerCase());
      seen.add(ref.dtag.toLowerCase());
    } else if (ref.kind === KIND_PERSONA) {
      label = personasById.get(ref.dtag);
    }
    rows.push({
      key: addr,
      label: label ?? ref.dtag,
      role: null,
      source: "published",
    });
  }
  const address = project.address?.toLowerCase() ?? null;
  if (address) {
    for (const agent of localAgents) {
      if (agent.projectRef?.toLowerCase() !== address) continue;
      const pubkey = agent.pubkey.toLowerCase();
      if (seen.has(pubkey)) continue;
      seen.add(pubkey);
      rows.push({
        key: `local:${pubkey}`,
        label: agent.name,
        role: agent.homeRole?.trim() || null,
        source: "local",
      });
    }
  }
  return rows;
}

/**
 * What the Agents card's number is a number OF. A card whose rows are all
 * local says so rather than claiming to count the project (ledger 207(3)).
 */
export function projectAgentCountLabel(
  rows: readonly ProjectAgentRow[],
): string {
  if (rows.length === 0) return "0";
  const local = rows.filter((row) => row.source === "local").length;
  if (local === rows.length) return `${rows.length} on this computer`;
  if (local === 0) return `${rows.length}`;
  return `${rows.length} · ${local} on this computer`;
}

/**
 * What the card says when it has no rows at all — never "No agents in this
 * project", which asserts a fact about the project that a screen reading
 * only published curation and this computer's own store cannot establish.
 */
export const PROJECT_AGENTS_EMPTY_HINT =
  "No agents for this project on this computer, and none published on the project.";

/**
 * One row in a project's sidebar child list; the type picks the icon. The
 * sidebar lists channels, interactive work (coding sessions and terminals)
 * and the to-do lists members pinned — repositories, workflows, agents and
 * Pulse live on the project page, not here.
 */
export type ProjectChildRow =
  | { type: "coding-session"; entry: ProjectCodingSessionShelfEntry }
  | { type: "channel"; channel: Channel }
  | { type: "forum"; channel: Channel }
  | { type: "shell"; session: ShellSessionInfo }
  | { type: "remote-shell"; terminal: RemoteTerminal }
  | { type: "todo-list"; list: TodoList };

/** Fixed display order of the flat list — live coding sessions on top: a
 * session is the only child that changes while you watch it. */
export const PROJECT_CHILD_TYPE_RANK: Record<ProjectChildRow["type"], number> =
  {
    "coding-session": 0,
    channel: 1,
    forum: 2,
    shell: 3,
    "remote-shell": 4,
    "todo-list": 5,
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
    case "todo-list":
      return `todo-list:${row.list.id}`;
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
    case "todo-list":
      return row.list.title;
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
  todoLists?: readonly TodoList[];
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
    ...(input.todoLists ?? []).map(
      (list): ProjectChildRow => ({ type: "todo-list", list }),
    ),
  ];
  return rows.sort(compareProjectChildren);
}
