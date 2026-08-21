import {
  KIND_MANAGED_AGENT,
  KIND_PERSONA,
  KIND_PROJECT,
  KIND_REPO_ANNOUNCEMENT,
  KIND_TEAM,
} from "@/shared/constants/kinds";
import type { RelayEvent } from "@/shared/api/types";

/**
 * A Buzz project container (kind:30621, NIP-MP) — a shared, owner-authored grouping
 * of Agents, Channels, Code (NIP-34 repos), and Forums. Addressed by
 * `(owner, dtag)`; `address` is the NIP-33 coordinate `30621:<owner>:<dtag>`.
 *
 * Membership is hybrid: `repoAddrs`/`agentAddrs`/`channelIds` are the owner's
 * curated forward references from the project event itself; items created by
 * other members back-reference the project (repo `project` tag, channel
 * `project_ref`) and are unioned in at read time.
 */
export type ProjectContainer = {
  id: string;
  dtag: string;
  owner: string;
  name: string;
  description: string;
  createdAt: number;
  address: string;
  repoAddrs: string[];
  agentAddrs: string[];
  channelIds: string[];
};

/** Reserved dtag for the auto-created default project. */
export const GENERAL_PROJECT_DTAG = "general";

/**
 * Canonicalizes a project coordinate: a `kind:owner:dtag` back-reference
 * resolves to the project's NIP-33 address. Returns null for non-project
 * coordinates.
 */
export function normalizeProjectRef(value: string): string | null {
  const ref = parseMemberRef(value);
  if (!ref) return null;
  if (ref.kind !== KIND_PROJECT) {
    return null;
  }
  return projectContainerAddress(ref.owner, ref.dtag);
}

/** Route/sidebar id for the locally-synthesized General bucket shown before
 * the workspace owner has published a real `general` project event. */
export const LOCAL_GENERAL_ID = "local:general";

export function makeLocalGeneral(): ProjectContainer {
  return {
    id: LOCAL_GENERAL_ID,
    dtag: GENERAL_PROJECT_DTAG,
    owner: "",
    name: "General",
    description: "",
    createdAt: 0,
    address: projectContainerAddress("", GENERAL_PROJECT_DTAG),
    repoAddrs: [],
    agentAddrs: [],
    channelIds: [],
  };
}

/**
 * The projects a surface should display: the published containers, with the
 * local General placeholder prepended until the workspace owner publishes a
 * real `general` project — so unclaimed items always have a home.
 */
export function displayProjectsWithGeneral(
  projects: ProjectContainer[],
): ProjectContainer[] {
  const hasGeneral = projects.some(
    (project) => project.dtag === GENERAL_PROJECT_DTAG,
  );
  return hasGeneral ? projects : [makeLocalGeneral(), ...projects];
}

export type ProjectMemberRef = {
  kind: number;
  owner: string;
  dtag: string;
};

export function projectContainerAddress(owner: string, dtag: string): string {
  return `${KIND_PROJECT}:${owner}:${dtag}`;
}

/**
 * Parses a `kind:owner:dtag` coordinate. Returns null when the shape is not
 * a valid member/project reference (non-numeric kind, malformed pubkey,
 * empty dtag).
 */
export function parseMemberRef(value: string): ProjectMemberRef | null {
  const first = value.indexOf(":");
  const second = value.indexOf(":", first + 1);
  if (first <= 0 || second <= first) return null;
  const kind = Number(value.slice(0, first));
  const owner = value.slice(first + 1, second);
  const dtag = value.slice(second + 1);
  if (!Number.isInteger(kind) || kind <= 0) return null;
  if (!/^[0-9a-fA-F]{64}$/.test(owner)) return null;
  if (dtag.length === 0) return null;
  return { kind, owner: owner.toLowerCase(), dtag };
}

const AGENT_MEMBER_KINDS = new Set([
  KIND_PERSONA,
  KIND_TEAM,
  KIND_MANAGED_AGENT,
]);

function getTag(event: RelayEvent, name: string): string | undefined {
  const value = event.tags.find((t) => t[0] === name)?.[1];
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function getAllTags(event: RelayEvent, name: string): string[] {
  return event.tags
    .filter((t) => t[0] === name && typeof t[1] === "string" && t[1].length > 0)
    .map((t) => t[1]);
}

/**
 * Converts a kind:30621 project event into a `ProjectContainer`. Returns null
 * for events without a `d` tag (they cannot be addressed). Unknown-kind `a`
 * tags are dropped rather than failing the whole event, so an older client
 * still renders projects curated by a newer one.
 */
export function eventToProjectContainer(
  event: RelayEvent,
): ProjectContainer | null {
  const dtag = getTag(event, "d");
  if (!dtag) return null;
  const owner = event.pubkey.toLowerCase();
  const repoAddrs: string[] = [];
  const agentAddrs: string[] = [];
  for (const value of getAllTags(event, "a")) {
    const ref = parseMemberRef(value);
    if (!ref) continue;
    if (ref.kind === KIND_REPO_ANNOUNCEMENT) {
      repoAddrs.push(value);
    } else if (AGENT_MEMBER_KINDS.has(ref.kind)) {
      agentAddrs.push(value);
    }
  }
  return {
    id: `${owner}:${dtag}`,
    dtag,
    owner,
    name: getTag(event, "name") || dtag,
    description: getTag(event, "description") || event.content || "",
    createdAt: event.created_at,
    address: projectContainerAddress(owner, dtag),
    repoAddrs: [...new Set(repoAddrs)],
    agentAddrs: [...new Set(agentAddrs)],
    channelIds: [...new Set(getAllTags(event, "channel"))],
  };
}

/**
 * NIP-33 dedup: keep the newest event per `(pubkey, d)` slot.
 */
export function dedupProjectEvents(events: RelayEvent[]): RelayEvent[] {
  const best = new Map<string, RelayEvent>();
  for (const event of events) {
    const key = `${event.pubkey.toLowerCase()}:${getTag(event, "d") ?? ""}`;
    const prev = best.get(key);
    if (!prev || event.created_at > prev.created_at) {
      best.set(key, event);
    }
  }
  return [...best.values()];
}

/**
 * NIP-09: a project is deleted only when a deletion event signed by the
 * project owner `a`-references its coordinate.
 */
export function isProjectContainerDeleted(
  project: ProjectContainer,
  deletionEvents: RelayEvent[],
): boolean {
  return deletionEvents.some(
    (event) =>
      event.pubkey.toLowerCase() === project.owner &&
      event.tags.some((tag) => tag[0] === "a" && tag[1] === project.address),
  );
}

/**
 * Assigns each item to the projects that claim it and returns the leftovers.
 *
 * `claims` maps a project to the set of keys it claims (forward refs);
 * `backRef` extracts an item's own claim (e.g. a repo's `project` tag or a
 * channel's `project_ref`), matched against project addresses. An item
 * claimed by several projects appears under each (union semantics) — except
 * General: it is the fallback bucket, not a competing curation target, so a
 * stale ref on the General event (e.g. left behind by its sweep, or by a
 * move the General owner's identity never reconciled) must not duplicate an
 * item that a real project claims.
 */
export function partitionByProject<T>(
  projects: ProjectContainer[],
  items: T[],
  itemKey: (item: T) => string,
  claims: (project: ProjectContainer) => readonly string[],
  backRef?: (item: T) => string | null | undefined,
): { byProject: Map<string, T[]>; unclaimed: T[] } {
  const byProject = new Map<string, T[]>(
    projects.map((project) => [project.id, []]),
  );
  const byAddress = new Map(
    projects.map((project) => [project.address, project]),
  );
  const claimSets = projects.map(
    (project) => [project, new Set(claims(project))] as const,
  );
  const generalIds = new Set(
    projects
      .filter((project) => project.dtag === GENERAL_PROJECT_DTAG)
      .map((project) => project.id),
  );

  const unclaimed: T[] = [];
  for (const item of items) {
    const key = itemKey(item);
    const owners = new Set<string>();
    for (const [project, claimed] of claimSets) {
      if (claimed.has(key)) owners.add(project.id);
    }
    const ref = backRef?.(item);
    if (ref) {
      const project = byAddress.get(normalizeProjectRef(ref) ?? ref);
      if (project) owners.add(project.id);
    }
    if (owners.size === 0) {
      unclaimed.push(item);
      continue;
    }
    const claimedByRealProject = [...owners].some((id) => !generalIds.has(id));
    if (claimedByRealProject) {
      for (const id of generalIds) owners.delete(id);
    }
    for (const id of owners) {
      byProject.get(id)?.push(item);
    }
  }
  return { byProject, unclaimed };
}

/**
 * Collapse duplicate `general` projects (possible on relays without a
 * membership system, where any client may publish the default project) to a
 * single canonical head: the oldest by `created_at`, tie-broken by owner so
 * every client converges on the same one.
 */
export function canonicalizeProjectContainers(
  projects: ProjectContainer[],
): ProjectContainer[] {
  const generals = projects.filter(
    (project) => project.dtag === GENERAL_PROJECT_DTAG,
  );
  if (generals.length <= 1) return projects;
  const canonical = generals.reduce((best, candidate) =>
    candidate.createdAt < best.createdAt ||
    (candidate.createdAt === best.createdAt && candidate.owner < best.owner)
      ? candidate
      : best,
  );
  return projects.filter(
    (project) =>
      project.dtag !== GENERAL_PROJECT_DTAG || project.id === canonical.id,
  );
}

/**
 * Groups channel-scoped items (e.g. workflows — kind 30620 always carries an
 * `h` channel tag) by the project owning their channel. Items on channels no
 * project claims are returned as `unclaimed` (displayed under General).
 */
export function partitionByChannelProject<
  T extends { channelId: string | null },
>(
  items: readonly T[],
  channelIdToProjectId: ReadonlyMap<string, string>,
): { byProject: Map<string, T[]>; unclaimed: T[] } {
  const byProject = new Map<string, T[]>();
  const unclaimed: T[] = [];
  for (const item of items) {
    const projectId = item.channelId
      ? channelIdToProjectId.get(item.channelId)
      : undefined;
    if (projectId === undefined) {
      unclaimed.push(item);
      continue;
    }
    const bucket = byProject.get(projectId);
    if (bucket) {
      bucket.push(item);
    } else {
      byProject.set(projectId, [item]);
    }
  }
  return { byProject, unclaimed };
}

/** Sort projects for display: General first, then by creation time. */
export function sortProjectContainers(
  projects: ProjectContainer[],
): ProjectContainer[] {
  return [...projects].sort((a, b) => {
    const aGeneral = a.dtag === GENERAL_PROJECT_DTAG ? 0 : 1;
    const bGeneral = b.dtag === GENERAL_PROJECT_DTAG ? 0 : 1;
    if (aGeneral !== bGeneral) return aGeneral - bGeneral;
    if (a.createdAt !== b.createdAt) return a.createdAt - b.createdAt;
    return a.id.localeCompare(b.id);
  });
}
