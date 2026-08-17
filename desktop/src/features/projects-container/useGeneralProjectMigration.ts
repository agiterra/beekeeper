import { useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import { fetchProjects } from "@/features/projects/hooks";
import { getChannels } from "@/shared/api/tauriChannels";
import { getIdentity } from "@/shared/api/tauriIdentity";
import { useRelayConnection } from "@/shared/api/useRelayConnection";

import {
  fetchAndSnapshotProjectContainers,
  fetchProjectContainers,
  projectContainersQueryKey,
  projectContainersQueryKeyFor,
  type ProjectContainer,
} from "./hooks";

import {
  GENERAL_PROJECT_DTAG,
  LOCAL_GENERAL_ID,
  makeLocalGeneral,
} from "./lib/projectContainerModel";
import {
  addProjectMembers,
  publishProjectContainer,
} from "./useCreateProjectContainer";

/** Scopes (pubkey:relay) the migration already ran for in this process. */
const migratedScopes = new Set<string>();

/**
 * Publish the default "General" project, sweeping in existing repos and
 * non-DM channels (streams and forums both must belong to a project) as
 * owner-curated refs. Keyed by the fixed `(self, 30621, "general")` slot so
 * repeated runs converge (NIP-33 LWW). Shared by the one-shot migration and
 * the lazy create-inside-General path.
 *
 * The sweep takes only what no real project owns: an item already claimed by
 * (or back-referencing) a non-General project must not be re-claimed here — a
 * General ref outlives later moves whenever the mover isn't the General
 * owner, leaving the item duplicated under General and its project.
 */
export async function publishGeneralProject(): Promise<ProjectContainer> {
  const [repos, channels, containers] = await Promise.all([
    fetchProjects().catch(() => []),
    getChannels(null)
      .then((result) => result.channels ?? [])
      .catch(() => []),
    fetchProjectContainers().catch(() => []),
  ]);

  const realProjects = containers.filter(
    (project) => project.dtag !== GENERAL_PROJECT_DTAG,
  );
  const claimedRepos = new Set(
    realProjects.flatMap((project) => project.repoAddrs),
  );
  const claimedChannels = new Set(
    realProjects.flatMap((project) => project.channelIds),
  );

  return publishProjectContainer({
    name: "General",
    dtag: GENERAL_PROJECT_DTAG,
    description: "Default project for existing work.",
    extraTags: [
      ...repos
        .flatMap((project) => project.repositories)
        .filter(
          (repo) => !repo.projectRef && !claimedRepos.has(repo.repoAddress),
        )
        .map((repo) => ["a", repo.repoAddress]),
      ...channels
        .filter(
          (channel) =>
            channel.channelType !== "dm" &&
            !channel.projectRef &&
            !claimedChannels.has(channel.id),
        )
        .map((channel) => ["channel", channel.id]),
    ],
  });
}

/**
 * Resolve a possibly-local project to a real, published one: the local
 * General placeholder is published first (self-owned, with the repo/forum
 * sweep) so new references have a valid coordinate. Real projects pass
 * through unchanged.
 */
export async function ensureRealProject(
  project: ProjectContainer,
): Promise<ProjectContainer> {
  if (project.id !== LOCAL_GENERAL_ID) return project;
  return publishGeneralProject();
}

/**
 * Returns a resolver for the General project's coordinate, publishing the
 * real General first when only the local placeholder exists. Used by create
 * flows that must attach a `projectRef` (channels must belong to a project).
 */
export function useGeneralProjectRefResolver(
  projects: ProjectContainer[],
  enabled: boolean,
): () => Promise<string | undefined> {
  return React.useCallback(async () => {
    if (!enabled) return undefined;
    const general = await ensureRealProject(
      projects.find((project) => project.dtag === GENERAL_PROJECT_DTAG) ??
        makeLocalGeneral(),
    );
    return general.address;
  }, [projects, enabled]);
}

/**
 * One-shot "General" project migration for the Projects experiment.
 *
 * The first client to notice no `general` project exists publishes it
 * (first-writer-wins, any role — in practice the provisioning "owner"
 * identity is often not the one people actually run, so an owner-only gate
 * just left everyone on the local placeholder). Concurrent publishes from
 * different identities are safe: the fetch layer canonicalizes duplicate
 * generals to the oldest head, so every client converges on one.
 */
export function useGeneralProjectMigration(
  enabled: boolean,
  relayUrl: string | undefined,
): void {
  const queryClient = useQueryClient();
  // The sidebar mounts instantly from the cached channel snapshot — often
  // before the relay socket has connected/authed. Gate the one-shot attempt
  // on a live connection, and re-attempt (per the scope guard) whenever the
  // connection comes back.
  const connectionState = useRelayConnection();
  const connected = connectionState === "connected";

  React.useEffect(() => {
    if (!enabled || !relayUrl || !connected) return;
    let cancelled = false;

    void (async () => {
      const identity = await getIdentity().catch(() => null);
      if (!identity || cancelled) return;
      const selfPubkey = identity.pubkey.toLowerCase();
      const scope = `${selfPubkey}:${relayUrl}`;
      if (migratedScopes.has(scope)) return;
      migratedScopes.add(scope);

      try {
        // Share the sidebar's query cache instead of issuing a second
        // containers REQ at the exact boot moment the rate-limit gate is
        // already congested; a fresh or in-flight sidebar fetch is reused.
        const containers = await queryClient.fetchQuery({
          queryKey: projectContainersQueryKeyFor(relayUrl),
          queryFn: () =>
            fetchAndSnapshotProjectContainers(relayUrl, selfPubkey),
          staleTime: 60_000,
        });
        if (cancelled) return;
        let general = containers.find(
          (project) => project.dtag === GENERAL_PROJECT_DTAG,
        );

        if (!general) {
          general = await publishGeneralProject();
          void queryClient.invalidateQueries({
            queryKey: projectContainersQueryKey,
          });
        } else if (general.owner === selfPubkey) {
          // Channels must belong to a project: sweep any channel no project
          // claims into the existing General head (curated-ref union), so
          // membership is durable rather than display-only.
          const channels = await getChannels(null)
            .then((result) => result.channels ?? [])
            .catch(() => []);
          if (cancelled) return;
          const claimed = new Set(
            containers.flatMap((project) => project.channelIds),
          );
          const unclaimedIds = channels
            .filter(
              (channel) =>
                channel.channelType !== "dm" &&
                !channel.projectRef &&
                !claimed.has(channel.id),
            )
            .map((channel) => channel.id);
          if (unclaimedIds.length > 0) {
            general = await addProjectMembers(general, {
              channelIds: unclaimedIds,
            });
            void queryClient.invalidateQueries({
              queryKey: projectContainersQueryKey,
            });
          }
        }
      } catch (error) {
        // Best-effort migration: allow a retry on the next mount or
        // reconnect of this scope — but never silently.
        console.error("[projects] General project migration failed:", error);
        migratedScopes.delete(scope);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [enabled, relayUrl, queryClient, connected]);
}
