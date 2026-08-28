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
    members: input.members,
    icon: input.icon,
    color: input.color,
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
