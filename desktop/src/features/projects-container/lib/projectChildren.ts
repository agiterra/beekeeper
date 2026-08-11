import type { Channel } from "@/shared/api/types";
import type { Workflow } from "@/shared/api/workflowTypes";
import type { Repository as CodeRepo } from "@/features/projects/hooks";
import { KIND_MANAGED_AGENT, KIND_PERSONA } from "@/shared/constants/kinds";

import { parseMemberRef } from "./projectContainerModel";
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
  | { type: "channel"; channel: Channel }
  | { type: "forum"; channel: Channel }
  | { type: "repo"; repo: CodeRepo }
  | { type: "workflow"; workflow: Workflow }
  | { type: "agent"; agent: ProjectAgentRow };

/** Fixed display order of the flat list — mirrors the old subsection order,
 * with forums promoted next to channels. */
export const PROJECT_CHILD_TYPE_RANK: Record<ProjectChildRow["type"], number> =
  {
    channel: 0,
    forum: 1,
    repo: 2,
    workflow: 3,
    agent: 4,
  };

/** Stable, cross-type-unique React key for a child row. */
export function projectChildKey(row: ProjectChildRow): string {
  switch (row.type) {
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
  }
}

export function projectChildLabel(row: ProjectChildRow): string {
  switch (row.type) {
    case "channel":
    case "forum":
      return row.channel.name;
    case "repo":
      return row.repo.name;
    case "workflow":
      return row.workflow.name;
    case "agent":
      return row.agent.label;
  }
}

export function compareProjectChildren(
  a: ProjectChildRow,
  b: ProjectChildRow,
): number {
  const byRank =
    PROJECT_CHILD_TYPE_RANK[a.type] - PROJECT_CHILD_TYPE_RANK[b.type];
  if (byRank !== 0) return byRank;
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
  streamChannels: Channel[];
  forumChannels: Channel[];
  repos: CodeRepo[];
  workflows: Workflow[];
  agents: ProjectAgentRow[];
}): ProjectChildRow[] {
  const rows: ProjectChildRow[] = [
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
  ];
  return rows.sort(compareProjectChildren);
}
