import { useQuery } from "@tanstack/react-query";

import { relayClient } from "@/shared/api/relayClient";
import { useIdentityQuery } from "@/shared/api/hooks";
import { KIND_PROJECT_MEMBERS } from "@/shared/constants/kinds";

import type { ProjectMember } from "./projectContainerModel";
import {
  parseRosterEventMembers,
  useProjectRosterQuery,
} from "./projectMembers";
import type { ProjectContainer } from "../hooks";

/**
 * What the current viewer may do to a project and the things inside it.
 *
 * One tier, two ways in. Server-side the project *creator* — the pubkey the
 * kind:30621 head is addressed to — is an implicit `owner`
 * (`ProjectGate::role_of`, `crates/beekeeper-db/src/project_acl.rs`), and so is
 * anybody seated `owner` on the roster by a kind:9010 op. This module is the
 * client's single statement of that fact; before it, three screens compared
 * `project.owner === self` inline and a roster Owner saw no Delete at all.
 *
 * The one capability that really is creator-only is `canEditHead`. NIP-01
 * addresses a replaceable event by `(kind, pubkey, d)`, so a roster Owner
 * republishing the head would mint `30621:<them>:<slug>` — a *different*
 * project, not an edit of this one. That is NIP-MP v1's stated rule
 * (`docs/nips/NIP-MP.md`), and it is a fact about the protocol rather than a
 * policy we chose, which is why the settings dialog says so rather than
 * greying the fields in silence.
 *
 * Every value here is advisory. The relay re-decides each of these against
 * the ACL projection, and a client that got it wrong gets a refusal, not a
 * privilege.
 */
export type ProjectCapabilities = {
  /** The creator, or a roster `owner`. */
  isOwner: boolean;
  /** Delete the project itself, with or without its children. */
  canDeleteProject: boolean;
  /** Add, re-role and remove roster members (kind:9010/9011). */
  canManageRoster: boolean;
  /**
   * Rename the project, change its icon, colour or visibility. Creator-only
   * for the protocol reason above — not a permission we withhold.
   */
  canEditHead: boolean;
  /**
   * Delete one resource inside the project. Owners reach anything; everyone
   * else reaches only what they signed themselves, which is the same rule
   * the relay's NIP-09 authorship arm already enforces.
   *
   * `creatorPubkey` is the pubkey the resource is addressed to (a repo's
   * announcer, a terminal's owner, a session's founder). An absent or
   * unknown value denies — a delete affordance rendered over an unidentified
   * owner is a button that fails.
   */
  canDeleteResource: (creatorPubkey: string | null | undefined) => boolean;
  /**
   * The roster is still loading. Gate destructive *actions* on this, not just
   * their affordances: acting mid-load would act against a roster that has
   * collapsed toward empty.
   */
  isLoading: boolean;
};

/** Lowercased pubkey, or null when absent. */
function normalize(pubkey: string | null | undefined): string | null {
  return pubkey ? pubkey.toLowerCase() : null;
}

/**
 * The ownership decision itself, with no data fetching — the creator, or a
 * roster `owner`. Shared by both hooks below so a list surface and a detail
 * surface can never disagree about the same project.
 *
 * The `project.owner` comparison is not redundant with the roster scan: the
 * creator is never *on* the roster. The relay's kind:39010 projection emits
 * one `p` tag per invited member and none for the creator, because a
 * membership op targeting the creator is refused outright
 * (`ProjectMemberOpRefusal::TargetsCreator`). Every client has to add them
 * back, which is what `rosterWithOwner` does for display and this does for
 * authority.
 */
export function viewerIsProjectOwner(
  self: string | null,
  project: Pick<ProjectContainer, "owner"> | null,
  roster: readonly ProjectMember[],
): boolean {
  if (!self || !project?.owner) return false;
  if (normalize(project.owner) === self) return true;
  return roster.some(
    (member) => normalize(member.pubkey) === self && member.role === "owner",
  );
}

function capabilitiesFor(
  self: string | null,
  project: ProjectContainer | null,
  roster: readonly ProjectMember[],
  isLoading: boolean,
): ProjectCapabilities {
  const isOwner = viewerIsProjectOwner(self, project, roster);
  return {
    isOwner,
    canDeleteProject: isOwner,
    canManageRoster: isOwner,
    canEditHead:
      !!self && !!project?.owner && normalize(project.owner) === self,
    canDeleteResource: (creatorPubkey) =>
      isOwner || (!!self && normalize(creatorPubkey) === self),
    isLoading,
  };
}

/**
 * Resolve the viewer's capabilities on one project.
 *
 * Reads the same roster the members table does — the relay-signed kind:39010
 * projection, falling back to the head's `p` tags before the first membership
 * op (`useProjectRosterQuery`).
 *
 * Pass `null` for the local General placeholder and anything else without a
 * published head: it has no coordinate to hold a roster, and every capability
 * comes back false.
 */
export function useProjectCapabilities(
  project: ProjectContainer | null,
): ProjectCapabilities {
  const identityQuery = useIdentityQuery();
  const self = normalize(identityQuery.data?.pubkey);
  const rosterQuery = useProjectRosterQuery(project);
  const roster = rosterQuery.data ?? project?.members ?? [];
  return capabilitiesFor(
    self,
    project,
    roster,
    identityQuery.isLoading || rosterQuery.isLoading,
  );
}

/** Query key for the batched roster read below. */
export function projectRosterBatchQueryKey(addresses: readonly string[]) {
  return ["project-roster-batch", [...addresses].sort().join(",")] as const;
}

/**
 * Capabilities for *many* projects, resolved in one relay round trip.
 *
 * A list surface renders one card per project and cannot call
 * [`useProjectCapabilities`] per row — that is a hook inside a loop, and it
 * would also fan out one REQ per project. This fetches every kind:39010
 * projection in a single `#d`-filtered query and indexes them by coordinate.
 *
 * The per-project fallback is deliberately identical to the single-project
 * path: a project with no projection yet (nobody has run a 9010/9011 op
 * against it) is judged on its head's own `p` tags, which is what
 * `fetchProjectRoster` does. Diverging here would mean a project's Delete
 * item appeared on one screen and not the other.
 */
export function useProjectCapabilitiesMap(
  projects: readonly ProjectContainer[],
): {
  /** Capabilities for one of the projects passed in. */
  capabilitiesFor: (project: ProjectContainer) => ProjectCapabilities;
  isLoading: boolean;
} {
  const identityQuery = useIdentityQuery();
  const self = normalize(identityQuery.data?.pubkey);
  const addresses = projects
    .filter((project) => project.owner.length > 0)
    .map((project) => project.address);

  const rostersQuery = useQuery({
    enabled: addresses.length > 0,
    queryKey: projectRosterBatchQueryKey(addresses),
    queryFn: async () => {
      const events = await relayClient.fetchEvents({
        kinds: [KIND_PROJECT_MEMBERS],
        "#d": addresses,
        // One projection per project, and the relay replaces rather than
        // appends, so this only has to be generous enough to survive a
        // replacement racing the read.
        limit: addresses.length * 2,
      });
      const latest = new Map<
        string,
        { at: number; members: ProjectMember[] }
      >();
      for (const event of events) {
        const address = event.tags.find((tag) => tag[0] === "d")?.[1];
        if (!address) continue;
        const seen = latest.get(address);
        if (seen && seen.at >= event.created_at) continue;
        latest.set(address, {
          at: event.created_at,
          members: parseRosterEventMembers(event),
        });
      }
      return new Map(
        [...latest].map(([address, { members }]) => [address, members]),
      );
    },
    staleTime: 30_000,
  });

  const isLoading = identityQuery.isLoading || rostersQuery.isLoading;
  return {
    capabilitiesFor: (project) =>
      capabilitiesFor(
        self,
        project,
        rostersQuery.data?.get(project.address) ?? project.members,
        isLoading,
      ),
    isLoading,
  };
}
