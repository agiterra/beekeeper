import { useMutation, useQueryClient } from "@tanstack/react-query";

import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import { getIdentity } from "@/shared/api/tauriIdentity";
import { KIND_PROJECT } from "@/shared/constants/kinds";

import {
  fetchProjectContainers,
  projectContainersQueryKey,
  type ProjectContainer,
} from "./hooks";
import { eventToProjectContainer } from "./lib/projectContainerModel";

export type CreateProjectContainerInput = {
  name: string;
  description?: string;
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
 */
export async function publishProjectContainer(input: {
  name: string;
  dtag: string;
  description?: string;
  extraTags?: string[][];
  createdAt?: number;
}): Promise<ProjectContainer> {
  const tags: string[][] = [
    ["d", input.dtag],
    ["name", input.name],
  ];
  const description = input.description?.trim() ?? "";
  if (description) {
    tags.push(["description", description]);
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

async function createProjectContainer(
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

  const identity = await getIdentity();
  const ownerPubkey = identity.pubkey.toLowerCase();
  const existing = await fetchProjectContainers();
  if (
    existing.some(
      (project) => project.owner === ownerPubkey && project.dtag === dtag,
    )
  ) {
    throw new Error(`You already have a project named "${dtag}".`);
  }

  return publishProjectContainer({
    name,
    dtag,
    description: input.description,
  });
}

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
    mutationFn: createProjectContainer,
    onSuccess: (project) => {
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
