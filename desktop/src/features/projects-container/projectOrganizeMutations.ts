import { useMutation, useQueryClient } from "@tanstack/react-query";

import { channelsQueryKey } from "@/features/channels/hooks";
import {
  projectsQueryKey,
  type Repository as CodeRepo,
} from "@/features/projects/hooks";
import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import { deleteChannel, updateChannel } from "@/shared/api/tauriChannels";
import { getIdentity } from "@/shared/api/tauriIdentity";
import { deleteWorkflow } from "@/shared/api/tauriWorkflows";
import type { Channel } from "@/shared/api/types";
import {
  KIND_DELETION,
  KIND_REPO_ANNOUNCEMENT,
} from "@/shared/constants/kinds";

import { projectContainersQueryKey, type ProjectContainer } from "./hooks";
import type { ProjectCascadeTargets } from "./lib/projectCascade";
import {
  addProjectMembers,
  publishProjectContainer,
  removeProjectMembers,
} from "./useCreateProjectContainer";
import { ensureRealProject } from "./useGeneralProjectMigration";

async function selfPubkey(): Promise<string> {
  const identity = await getIdentity();
  return identity.pubkey.toLowerCase();
}

/**
 * Owner-curated forward-ref reconciliation shared by the move mutations:
 * add the item to the target project's list and drop it from the source's,
 * for whichever of the two the current identity owns. Returns true when at
 * least one list was updated.
 */
async function reconcileForwardRefs(
  self: string,
  refs: { channelIds?: string[]; repoAddrs?: string[] },
  from: ProjectContainer | null,
  to: ProjectContainer,
): Promise<boolean> {
  let updated = false;
  if (to.owner === self) {
    await addProjectMembers(to, refs);
    updated = true;
  }
  if (from && from.owner === self && from.id !== to.id) {
    await removeProjectMembers(from, refs);
    updated = true;
  }
  return updated;
}

export type MoveChannelToProjectInput = {
  channel: Channel;
  /** The project currently displaying the channel, if any. */
  from: ProjectContainer | null;
  /** Target project, or `null` to make the channel global (streams only). */
  to: ProjectContainer | null;
};

/**
 * Move a channel or forum into a project (or back to global with
 * `to: null`): back-reference via a kind:9002 `project` edit (needs channel
 * owner/admin and a relay that understands the tag) plus owner-curated
 * forward refs on the affected project events. Fails only when neither path
 * applied.
 */
async function moveChannelToProject({
  channel,
  from,
  to,
}: MoveChannelToProjectInput): Promise<void> {
  const target = to ? await ensureRealProject(to) : null;
  const self = await selfPubkey();

  let backRefApplied = true;
  try {
    await updateChannel({
      channelId: channel.id,
      project: target?.address ?? null,
    });
  } catch {
    backRefApplied = false;
  }

  let forwardRefApplied = false;
  if (target) {
    forwardRefApplied = await reconcileForwardRefs(
      self,
      { channelIds: [channel.id] },
      from,
      target,
    );
  } else if (from && from.owner === self) {
    await removeProjectMembers(from, { channelIds: [channel.id] });
    forwardRefApplied = true;
  }

  if (!backRefApplied && !forwardRefApplied) {
    throw new Error(
      "You need to be a channel admin or the project owner to move this channel.",
    );
  }
}

export function useMoveChannelToProjectMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: moveChannelToProject,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: channelsQueryKey });
      void queryClient.invalidateQueries({
        queryKey: projectContainersQueryKey,
      });
    },
  });
}

export type MoveRepoToProjectInput = {
  repo: CodeRepo;
  from: ProjectContainer | null;
  to: ProjectContainer;
};

/**
 * Move a repo into a project. Self-owned repos republish their kind:30617
 * announcement with the `project` tag replaced (all other tags preserved,
 * except relay-maintained `auth` grants — mirroring the CLI's
 * read-modify-write in `buzz-cli`); forward refs cover repos owned by others.
 */
async function moveRepoToProject({
  repo,
  from,
  to,
}: MoveRepoToProjectInput): Promise<void> {
  const target = await ensureRealProject(to);
  const self = await selfPubkey();

  let backRefApplied = false;
  if (repo.owner.toLowerCase() === self) {
    const events = await relayClient.fetchEvents({
      kinds: [KIND_REPO_ANNOUNCEMENT],
      authors: [repo.owner],
      "#d": [repo.dtag],
      limit: 5,
    });
    const latest = [...events].sort((a, b) => b.created_at - a.created_at)[0];
    if (latest) {
      const tags = latest.tags.filter(
        (tag) => tag[0] !== "project" && tag[0] !== "auth",
      );
      tags.push(["project", target.address]);
      const event = await signRelayEvent({
        kind: KIND_REPO_ANNOUNCEMENT,
        content: latest.content,
        tags,
      });
      await relayClient.publishEvent(
        event,
        "Timed out moving repository.",
        "Failed to move repository.",
      );
      backRefApplied = true;
    }
  }

  const forwardRefApplied = await reconcileForwardRefs(
    self,
    { repoAddrs: [repo.repoAddress] },
    from,
    target,
  );

  if (!backRefApplied && !forwardRefApplied) {
    throw new Error(
      "You need to own the repository or the project to move it.",
    );
  }
}

export function useMoveRepoToProjectMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: moveRepoToProject,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: projectsQueryKey });
      void queryClient.invalidateQueries({
        queryKey: projectContainersQueryKey,
      });
    },
  });
}

export type UpdateProjectContainerInput = {
  project: ProjectContainer;
  name: string;
  description?: string;
  /** Defaults to the project's current visibility when omitted. */
  visibility?: ProjectContainer["visibility"];
  /** Defaults to the project's current members when omitted. Members are
   * kept across visibility changes — they carry roles, not just a
   * private-project ACL. */
  members?: ProjectContainer["members"];
  /** undefined = keep the project's current icon; null = remove it. */
  icon?: string | null;
  /** undefined = keep the project's current color; null = remove it. */
  color?: string | null;
};

/** Rename/edit a project the current identity owns (same-dtag republish).
 * Exported (in addition to the mutation hook below) so the
 * visibility/member-passthrough regression is directly unit-testable. */
export async function updateProjectContainer({
  project,
  name,
  description,
  visibility,
  members,
  icon,
  color,
}: UpdateProjectContainerInput): Promise<ProjectContainer> {
  const self = await selfPubkey();
  if (project.owner !== self) {
    throw new Error("Only the project owner can edit it.");
  }
  const trimmed = name.trim();
  if (!trimmed) {
    throw new Error("Project name is required.");
  }
  const nextVisibility = visibility ?? project.visibility;
  return publishProjectContainer({
    name: trimmed,
    dtag: project.dtag,
    description: description?.trim() ?? "",
    visibility: nextVisibility,
    members: members ?? project.members,
    icon: icon === undefined ? project.icon : icon,
    color: color === undefined ? project.color : color,
    extraTags: [
      ...project.repoAddrs.map((addr) => ["a", addr]),
      ...project.agentAddrs.map((addr) => ["a", addr]),
      ...project.channelIds.map((id) => ["channel", id]),
    ],
  });
}

export function useUpdateProjectContainerMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: updateProjectContainer,
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: projectContainersQueryKey,
      });
    },
  });
}

/**
 * Delete a project the current identity owns (NIP-09 deletion of the 30621
 * event). Items referencing it fall back to the General bucket via the
 * unclaimed-items display rule.
 */
async function deleteProjectContainer(
  project: ProjectContainer,
): Promise<void> {
  const self = await selfPubkey();
  if (project.owner !== self) {
    throw new Error("Only the project owner can delete it.");
  }
  const event = await signRelayEvent({
    kind: KIND_DELETION,
    content: `Delete project ${project.name}`,
    tags: [["a", project.address]],
  });
  await relayClient.publishEvent(
    event,
    "Timed out deleting project.",
    "Failed to delete project.",
  );
}

export function useDeleteProjectContainerMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: deleteProjectContainer,
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: projectContainersQueryKey,
      });
      void queryClient.invalidateQueries({ queryKey: channelsQueryKey });
      void queryClient.invalidateQueries({ queryKey: projectsQueryKey });
    },
  });
}

export type DeleteProjectContainerCascadeInput = {
  project: ProjectContainer;
  /** Everything to delete alongside the project, from
   * `useProjectCascadeTargets`. */
  targets: ProjectCascadeTargets;
};

/**
 * Delete a project **and its channels and workflows** (opt-in only).
 *
 * NIP-MP says a project delete removes the kind:30621 event and nothing else,
 * and that stays the default everywhere — this path runs only when the user
 * ticks "delete everything" in the confirmation dialog.
 *
 * Order matters and mirrors the CLI's `bee projects delete --cascade`:
 * channels first, then workflows, and the project tombstone **last**. A
 * failure part-way through therefore leaves the project itself in place, so
 * the user can retry instead of being left with an unreachable orphan set.
 * Any child failure aborts before the tombstone and reports which children
 * survived — a partially-completed cascade is never reported as success.
 *
 * Repositories are never deleted, only detached: the relay keeps a repo's name
 * reservation so a deletion cannot free the name for another owner to claim.
 *
 * Only workflows the caller authored are deleted. `targets.workflows` is
 * already author-filtered by `useProjectCascadeTargets`, and this function
 * re-checks rather than trusting it: a workflow delete is a kind:5 `a`-tag
 * tombstone for `30620:<caller>:<id>`, so issuing one for a teammate's
 * workflow addresses a coordinate that does not exist. The relay accepts it,
 * matches no live row, and returns success — a delete that reports done and
 * changes nothing. Those are skipped here, and named to the user by the
 * dialog.
 */
export async function deleteProjectContainerCascade({
  project,
  targets,
}: DeleteProjectContainerCascadeInput): Promise<void> {
  const self = await selfPubkey();
  if (project.owner !== self) {
    throw new Error("Only the project owner can delete it.");
  }

  const failures: string[] = [];
  for (const channel of targets.channels) {
    try {
      await deleteChannel(channel.id);
    } catch {
      failures.push(`channel "${channel.name}"`);
    }
  }
  for (const workflow of targets.workflows) {
    // Belt and braces: never sign a tombstone that cannot delete anything.
    if (workflow.ownerPubkey.toLowerCase() !== self.toLowerCase()) continue;
    try {
      await deleteWorkflow(workflow.id);
    } catch {
      failures.push(`workflow "${workflow.name}"`);
    }
  }

  if (failures.length > 0) {
    throw new Error(
      `The project was NOT deleted: ${failures.join(", ")} could not be ` +
        `deleted. Fix those and try again.`,
    );
  }

  await deleteProjectContainer(project);
}

export function useDeleteProjectContainerCascadeMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: deleteProjectContainerCascade,
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: projectContainersQueryKey,
      });
      void queryClient.invalidateQueries({ queryKey: channelsQueryKey });
      void queryClient.invalidateQueries({ queryKey: projectsQueryKey });
    },
  });
}
