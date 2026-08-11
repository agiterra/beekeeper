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
import {
  eventToProjectContainer,
  GENERAL_PROJECT_DTAG,
  PROJECT_ACCESS_TAG,
} from "./lib/projectContainerModel";

export type CreateProjectContainerInput = {
  name: string;
  description?: string;
  visibility?: ProjectContainer["visibility"];
  memberPubkeys?: string[];
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
 * ⚠️ Rebuilds the visibility/`p`-member tags from `visibility`/`memberPubkeys`
 * on every call — every caller that republishes an existing project (add/
 * remove-member, organize mutations, the legacy-kind migration) MUST pass the
 * project's current `visibility`/`members` through, or the republish silently
 * drops them and a private project goes public.
 */
export async function publishProjectContainer(input: {
  name: string;
  dtag: string;
  description?: string;
  visibility?: ProjectContainer["visibility"];
  memberPubkeys?: string[];
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
  if (visibility === "private") {
    tags.push([PROJECT_ACCESS_TAG, "private"]);
    const members = [
      ...new Set(
        (input.memberPubkeys ?? [])
          .map((pubkey) => pubkey.toLowerCase())
          .filter(
            (pubkey) => /^[0-9a-f]{64}$/.test(pubkey) && pubkey !== ownerPubkey,
          ),
      ),
    ];
    tags.push(...members.map((pubkey) => ["p", pubkey]));
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
    visibility: input.visibility,
    memberPubkeys: input.memberPubkeys,
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
    visibility: project.visibility,
    memberPubkeys: project.members,
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
    memberPubkeys: project.members,
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
