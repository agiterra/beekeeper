import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_PROJECT_MEMBERS,
  KIND_PROJECT_PUT_MEMBER,
  KIND_PROJECT_REMOVE_MEMBER,
} from "@/shared/constants/kinds";
import { parseEntityRole } from "@/shared/lib/entityRoles";

import type { ProjectContainer, ProjectMember } from "./projectContainerModel";

/**
 * Project roster API — the invite-based member list with roles.
 *
 * The authoritative roster read is the relay-signed kind:39010 projection
 * (`d` = the project's `30621:<owner>:<dtag>` coordinate). Before the first
 * kind:9010/9011 membership op no projection exists and the head event's `p`
 * tags (already parsed into `project.members`) are the roster.
 *
 * Writes are user-signed ops: kind:9010 (put/upsert with role) and kind:9011
 * (remove). Only the project creator or a roster owner may send them; the
 * creator is an implicit Owner and never appears on the roster itself.
 */

const HEX64_REGEX = /^[0-9a-f]{64}$/;

/** A roster row for display: the implicit creator is prepended by
 * `rosterWithOwner` and flagged so the UI can pin it and hide management. */
export type ProjectRosterEntry = ProjectMember & { isCreator: boolean };

/**
 * Parses the `p` tags of a kind:39010 roster projection (or any p-carrying
 * membership event) into members. Role is tag element 3; role-less (legacy)
 * or unknown values read as collaborator — advisory display only, the relay
 * enforces the real grant.
 */
export function parseRosterEventMembers(event: RelayEvent): ProjectMember[] {
  const members: ProjectMember[] = [];
  const seen = new Set<string>();
  for (const tag of event.tags) {
    if (tag[0] !== "p" || typeof tag[1] !== "string") continue;
    const pubkey = tag[1].toLowerCase();
    if (!HEX64_REGEX.test(pubkey) || seen.has(pubkey)) continue;
    seen.add(pubkey);
    members.push({ pubkey, role: parseEntityRole(tag[3]) ?? "collaborator" });
  }
  return members;
}

/** Tags for a kind:9010 put-member op: `["a", coordinate]` plus one
 * `["p", <hex>, "", <role>]` (arity exactly 4, role required) per member. */
export function buildPutRosterTags(
  address: string,
  members: ProjectMember[],
): string[][] {
  return [
    ["a", address],
    ...members.map((member) => [
      "p",
      member.pubkey.toLowerCase(),
      "",
      member.role,
    ]),
  ];
}

/** Tags for a kind:9011 remove-member op: `["a", coordinate]` plus one
 * `["p", <hex>]` per removed pubkey. */
export function buildRemoveRosterTags(
  address: string,
  pubkeys: string[],
): string[][] {
  return [
    ["a", address],
    ...pubkeys.map((pubkey) => ["p", pubkey.toLowerCase()]),
  ];
}

/**
 * The full display roster: the implicit creator (always Owner, pinned first)
 * followed by the explicit roster rows. The creator can never be a roster
 * target, so any stray roster row with the creator's pubkey is dropped.
 */
export function rosterWithOwner(
  project: Pick<ProjectContainer, "owner">,
  roster: ProjectMember[],
): ProjectRosterEntry[] {
  const owner = project.owner.toLowerCase();
  const entries: ProjectRosterEntry[] = owner
    ? [{ pubkey: owner, role: "owner", isCreator: true }]
    : [];
  for (const member of roster) {
    if (member.pubkey.toLowerCase() === owner) continue;
    entries.push({ ...member, isCreator: false });
  }
  return entries;
}

/**
 * Fetches the authoritative roster: the kind:39010 projection when one
 * exists, otherwise the head event's `p`-tag members (pre-first-op projects,
 * or relays without the membership system).
 */
export async function fetchProjectRoster(
  project: ProjectContainer,
): Promise<ProjectMember[]> {
  // The local General placeholder has no owner/coordinate to query.
  if (!project.owner) return project.members;
  const events = await relayClient.fetchEvents({
    kinds: [KIND_PROJECT_MEMBERS],
    "#d": [project.address],
    limit: 1,
  });
  const latest = [...events].sort((a, b) => b.created_at - a.created_at)[0];
  if (latest) return parseRosterEventMembers(latest);
  return project.members;
}

/** Publishes a kind:9010 op adding members or changing their roles. */
export async function putProjectRosterMembers(
  project: ProjectContainer,
  members: ProjectMember[],
): Promise<void> {
  const event = await signRelayEvent({
    kind: KIND_PROJECT_PUT_MEMBER,
    content: "",
    tags: buildPutRosterTags(project.address, members),
  });
  await relayClient.publishEvent(
    event,
    "Timed out updating project members.",
    "Failed to update project members.",
  );
}

/** Publishes a kind:9011 op removing members from the roster. */
export async function removeProjectRosterMembers(
  project: ProjectContainer,
  pubkeys: string[],
): Promise<void> {
  const event = await signRelayEvent({
    kind: KIND_PROJECT_REMOVE_MEMBER,
    content: "",
    tags: buildRemoveRosterTags(project.address, pubkeys),
  });
  await relayClient.publishEvent(
    event,
    "Timed out removing project members.",
    "Failed to remove project members.",
  );
}

export function projectRosterQueryKey(address: string) {
  return ["project-roster", address] as const;
}

export function useProjectRosterQuery(project: ProjectContainer | null) {
  return useQuery({
    enabled: project !== null && project.owner.length > 0,
    queryKey: projectRosterQueryKey(project?.address ?? "none"),
    queryFn: () => {
      if (!project) throw new Error("No project.");
      return fetchProjectRoster(project);
    },
    staleTime: 30_000,
  });
}

/** Upsert members/roles on the roster, with optimistic cache update. */
export function usePutProjectRosterMutation(project: ProjectContainer) {
  const queryClient = useQueryClient();
  const queryKey = projectRosterQueryKey(project.address);

  return useMutation({
    mutationFn: (members: ProjectMember[]) =>
      putProjectRosterMembers(project, members),
    onMutate: async (members) => {
      await queryClient.cancelQueries({ queryKey });
      const previous = queryClient.getQueryData<ProjectMember[]>(queryKey);
      queryClient.setQueryData<ProjectMember[]>(queryKey, (old = []) => {
        const next = [...old];
        for (const member of members) {
          const pubkey = member.pubkey.toLowerCase();
          const index = next.findIndex((entry) => entry.pubkey === pubkey);
          if (index >= 0) {
            next[index] = { pubkey, role: member.role };
          } else {
            next.push({ pubkey, role: member.role });
          }
        }
        return next;
      });
      return { previous };
    },
    onError: (_err, _members, context) => {
      if (context?.previous) {
        queryClient.setQueryData(queryKey, context.previous);
      }
    },
    onSettled: async () => {
      await queryClient.invalidateQueries({ queryKey });
    },
  });
}

/** Remove members from the roster, with optimistic cache update. */
export function useRemoveProjectRosterMutation(project: ProjectContainer) {
  const queryClient = useQueryClient();
  const queryKey = projectRosterQueryKey(project.address);

  return useMutation({
    mutationFn: (pubkeys: string[]) =>
      removeProjectRosterMembers(project, pubkeys),
    onMutate: async (pubkeys) => {
      await queryClient.cancelQueries({ queryKey });
      const previous = queryClient.getQueryData<ProjectMember[]>(queryKey);
      const removed = new Set(pubkeys.map((pubkey) => pubkey.toLowerCase()));
      queryClient.setQueryData<ProjectMember[]>(queryKey, (old = []) =>
        old.filter((entry) => !removed.has(entry.pubkey)),
      );
      return { previous };
    },
    onError: (_err, _pubkeys, context) => {
      if (context?.previous) {
        queryClient.setQueryData(queryKey, context.previous);
      }
    },
    onSettled: async () => {
      await queryClient.invalidateQueries({ queryKey });
    },
  });
}
