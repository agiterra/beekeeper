import { useMutation, useQueryClient } from "@tanstack/react-query";

import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import { getIdentity } from "@/shared/api/tauriIdentity";
import {
  KIND_DELETION,
  KIND_PROJECT,
  KIND_REPO_ANNOUNCEMENT,
} from "@/shared/constants/kinds";

import { managedAgentsQueryKey } from "@/features/agents/hooks";

import { projectContainersQueryKey, type ProjectContainer } from "./hooks";
import {
  defaultAgentsRepoId,
  projectAgentsInit,
  type ProjectAgentsInitResult,
} from "./lib/projectAgentsInit";
import {
  eventToProjectContainer,
  GENERAL_PROJECT_DTAG,
  isProjectContainerDeleted,
  normalizeProjectColor,
  PROJECT_ACCESS_TAG,
  PROJECT_COLOR_TAG,
  PROJECT_ICON_TAG,
  type ProjectMember,
} from "./lib/projectContainerModel";

export type CreateProjectContainerInput = {
  name: string;
  description?: string;
  visibility?: ProjectContainer["visibility"];
  members?: ProjectMember[];
  /** Display emoji; absent/null publishes no `icon` tag. */
  icon?: string | null;
  /** Display tint (`#rrggbb`); absent/null publishes no `color` tag. */
  color?: string | null;
};

function slugFromName(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64);
}

/**
 * Publishes a project container event. `extraTags` carries the membership
 * refs (`a`/`channel`) — used by the General migration sweep; the plain
 * create dialog publishes with none.
 *
 * ⚠️ Rebuilds every tag from its input on each call — every caller that
 * republishes an existing project (add/remove-member, organize mutations, the
 * legacy-kind migration) MUST pass the project's current `visibility`/
 * `members`/`icon`/`color` through, or the republish silently drops them and
 * a private project goes public / loses its roster seed / loses its icon and
 * tint.
 *
 * Note the head's `p` tags are only the roster until the first kind:9010/9011
 * membership op — after that the relay sources the roster from ops and
 * ignores head `p` tags, so a republish can't clobber op-managed rosters.
 */
export async function publishProjectContainer(input: {
  name: string;
  dtag: string;
  description?: string;
  visibility?: ProjectContainer["visibility"];
  members?: ProjectMember[];
  icon?: string | null;
  color?: string | null;
  extraTags?: string[][];
  createdAt?: number;
}): Promise<ProjectContainer> {
  // The reserved `general` project must always stay public — guarded here
  // (not just at the UI layer) so no call site can accidentally privatize it.
  const isGeneral = input.dtag === GENERAL_PROJECT_DTAG;
  const visibility: ProjectContainer["visibility"] = isGeneral
    ? "public"
    : (input.visibility ?? "public");

  const identity = await getIdentity();
  const ownerPubkey = identity.pubkey.toLowerCase();

  const tags: string[][] = [
    ["d", input.dtag],
    ["name", input.name],
  ];
  const description = input.description?.trim() ?? "";
  if (description) {
    tags.push(["description", description]);
  }
  const icon = input.icon?.trim();
  if (icon) {
    tags.push([PROJECT_ICON_TAG, icon]);
  }
  const color = normalizeProjectColor(input.color);
  if (color) {
    tags.push([PROJECT_COLOR_TAG, color]);
  }
  if (visibility === "private") {
    tags.push([PROJECT_ACCESS_TAG, "private"]);
  }
  // Members are emitted regardless of visibility — they carry roles now, not
  // just a private-project ACL, so flipping a project public keeps them.
  const seenMembers = new Set<string>();
  for (const member of input.members ?? []) {
    const pubkey = member.pubkey.toLowerCase();
    if (!/^[0-9a-f]{64}$/.test(pubkey) || pubkey === ownerPubkey) continue;
    if (seenMembers.has(pubkey)) continue;
    seenMembers.add(pubkey);
    tags.push(["p", pubkey, "", member.role]);
  }
  tags.push(...(input.extraTags ?? []));

  const event = await signRelayEvent({
    kind: KIND_PROJECT,
    content: "",
    tags,
  });

  await relayClient.publishEvent(
    event,
    "Timed out creating project.",
    "Failed to create project.",
  );

  const project = eventToProjectContainer(event);
  if (!project) {
    throw new Error("Failed to read back the created project.");
  }
  return project;
}

/**
 * The two repository ids a project creates with it (spec § 4.11): the code
 * repository `<slug>` and the agents repository `<slug>-beekeeper-agents`.
 */
export function projectRepositoryIds(dtag: string): {
  code: string;
  agents: string;
} {
  return { code: dtag, agents: defaultAgentsRepoId(dtag) };
}

/**
 * The refusal when a repository id the project would create is already
 * announced by another key, or `null`. Repository ids are one namespace per
 * community (`buzz-db`'s `ReserveOutcome::TakenByOther`), so this is read
 * before the project is published: a project whose repositories can never
 * be created is not a project worth publishing.
 */
export function repositoryIdTakenRefusal(
  announcements: { pubkey: string; tags: string[][] }[],
  ownerPubkey: string,
  ids: string[],
): string | null {
  for (const event of announcements) {
    const author = event.pubkey.toLowerCase();
    if (author === ownerPubkey) continue;
    const id = event.tags.find((tag) => tag[0] === "d")?.[1];
    if (id === undefined || !ids.includes(id)) continue;
    return `Repository id "${id}" is already taken in this community by ${author.slice(0, 8)}…; repository ids are one namespace per community, so choose another project name.`;
  }
  return null;
}

/**
 * Create a project after checking its exact owned coordinate, and both
 * repository ids it will create, on the relay.
 */
export async function createProjectContainer(
  input: CreateProjectContainerInput,
): Promise<ProjectContainer> {
  const name = input.name.trim();
  if (!name) {
    throw new Error("Project name is required.");
  }
  const dtag = slugFromName(name);
  if (!dtag) {
    throw new Error("Project name must include letters or numbers.");
  }
  const repositoryIds = projectRepositoryIds(dtag);

  const identity = await getIdentity();
  const ownerPubkey = identity.pubkey.toLowerCase();
  // This is an explicit action, not project discovery. Read only the address
  // being created through the existing HTTP batch/admission path; broad WS
  // discovery can spend several read-budget windows before a write starts.
  // The two repository ids are read community-wide (any author): a hit by
  // another key refuses the whole create.
  const existing = await relayClient.fetchEventsBatch([
    { kinds: [KIND_PROJECT], authors: [ownerPubkey], "#d": [dtag], limit: 1 },
    {
      kinds: [KIND_DELETION],
      authors: [ownerPubkey],
      "#a": [`${KIND_PROJECT}:${ownerPubkey}:${dtag}`],
      limit: 1,
    },
    {
      kinds: [KIND_REPO_ANNOUNCEMENT],
      "#d": [repositoryIds.code, repositoryIds.agents],
      limit: 16,
    },
  ]);
  const taken = repositoryIdTakenRefusal(
    existing.filter((event) => event.kind === KIND_REPO_ANNOUNCEMENT),
    ownerPubkey,
    [repositoryIds.code, repositoryIds.agents],
  );
  if (taken !== null) {
    throw new Error(taken);
  }
  const deletions = existing.filter((event) => event.kind === KIND_DELETION);
  for (const event of existing) {
    if (
      event.kind !== KIND_PROJECT ||
      event.pubkey.toLowerCase() !== ownerPubkey ||
      !event.tags.some((tag) => tag[0] === "d" && tag[1] === dtag)
    )
      continue;
    const project = eventToProjectContainer(event);
    // A malformed head matching the requested d tag is not proof of absence.
    if (!project || project.dtag !== dtag) {
      throw new Error(
        `Could not verify whether project "${dtag}" already exists.`,
      );
    }
    if (!isProjectContainerDeleted(project, deletions)) {
      throw new Error(`You already have a project named "${dtag}".`);
    }
  }

  return publishProjectContainer({
    name,
    dtag,
    description: input.description,
    visibility: input.visibility,
    members: input.members,
    icon: input.icon,
    color: input.color,
  });
}

/**
 * Create, or finish creating, the project's repositories (spec § 4.11): one
 * host call, then a best-effort forward reference from the project head to
 * each repository that exists. The host's `complete`/`gap` verdict is
 * returned as is; the caller decides how to show it.
 */
export async function initProjectRepositories(
  project: ProjectContainer,
): Promise<ProjectAgentsInitResult> {
  const result = await projectAgentsInit({ projectRef: project.address });
  const identity = await getIdentity();
  if (project.owner === identity.pubkey.toLowerCase()) {
    const repoAddrs = [
      result.codeRepoExisted || result.codeAnnouncementEventId !== null
        ? result.codeRepoRef
        : null,
      result.agentsRepoExisted ||
      (result.agentsAnnouncementEventId !== null &&
        result.agentsAnnouncementWithdrawnEventId === null)
        ? result.agentsRepoRef
        : null,
    ].filter((addr): addr is string => addr !== null);
    if (repoAddrs.length > 0) {
      try {
        await addProjectMembers(project, { repoAddrs });
      } catch {
        // Each repository's own `project` back-reference still associates
        // it; the owner-curated forward ref is best-effort.
      }
    }
  }
  return result;
}

/** What creating a project produced: the head, and its repositories' fate. */
export type CreateProjectContainerOutcome = {
  project: ProjectContainer;
  /** The host's report, or `null` when the command itself failed. */
  repositories: ProjectAgentsInitResult | null;
  /** The command's own words when it threw (a refusal, no host, …). */
  repositoriesError: string | null;
};

/**
 * Republish a project the current identity owns with additional member
 * references (owner-curated forward refs). Rebuilds the event from the
 * parsed container fields, so only the owner should call this — publishing
 * under a different identity would fork the project rather than update it.
 */
export async function addProjectMembers(
  project: ProjectContainer,
  additions: { channelIds?: string[]; repoAddrs?: string[] },
): Promise<ProjectContainer> {
  const repoAddrs = [
    ...new Set([...project.repoAddrs, ...(additions.repoAddrs ?? [])]),
  ];
  const channelIds = [
    ...new Set([...project.channelIds, ...(additions.channelIds ?? [])]),
  ];
  return publishProjectContainer({
    name: project.name,
    dtag: project.dtag,
    description: project.description,
    visibility: project.visibility,
    members: project.members,
    icon: project.icon,
    color: project.color,
    extraTags: [
      ...repoAddrs.map((addr) => ["a", addr]),
      ...project.agentAddrs.map((addr) => ["a", addr]),
      ...channelIds.map((id) => ["channel", id]),
    ],
  });
}

/**
 * Republish a project the current identity owns with member references
 * removed. Sibling of `addProjectMembers` — owner-only for the same reason.
 */
export async function removeProjectMembers(
  project: ProjectContainer,
  removals: { channelIds?: string[]; repoAddrs?: string[] },
): Promise<ProjectContainer> {
  const dropRepos = new Set(removals.repoAddrs ?? []);
  const dropChannels = new Set(removals.channelIds ?? []);
  return publishProjectContainer({
    name: project.name,
    dtag: project.dtag,
    description: project.description,
    visibility: project.visibility,
    members: project.members,
    icon: project.icon,
    color: project.color,
    extraTags: [
      ...project.repoAddrs
        .filter((addr) => !dropRepos.has(addr))
        .map((addr) => ["a", addr]),
      ...project.agentAddrs.map((addr) => ["a", addr]),
      ...project.channelIds
        .filter((id) => !dropChannels.has(id))
        .map((id) => ["channel", id]),
    ],
  });
}

export function useCreateProjectContainerMutation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async (
      input: CreateProjectContainerInput,
    ): Promise<CreateProjectContainerOutcome> => {
      const project = await createProjectContainer(input);
      // The project exists once its head is published; the repositories
      // are a second step whose failure is disclosed, not a failed create
      // (Finish setup in Project settings re-runs it).
      try {
        return {
          project,
          repositories: await initProjectRepositories(project),
          repositoriesError: null,
        };
      } catch (thrown) {
        return {
          project,
          repositories: null,
          repositoriesError:
            thrown instanceof Error ? thrown.message : String(thrown),
        };
      }
    },
    onSuccess: ({ project, repositories }) => {
      // The create installed the project's default agents on this computer;
      // every agent picker reads the managed-agent list, so re-read it.
      if (repositories && repositories.agentsInstalled.length > 0) {
        void queryClient.invalidateQueries({ queryKey: managedAgentsQueryKey });
      }
      // Prefix-matched: the containers query is keyed per relay.
      queryClient.setQueriesData<ProjectContainer[]>(
        { queryKey: projectContainersQueryKey },
        (current = []) => [...current, project],
      );
      void queryClient.invalidateQueries({
        queryKey: projectContainersQueryKey,
      });
    },
  });
}
