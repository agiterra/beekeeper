import * as React from "react";
import { useQuery } from "@tanstack/react-query";

import { useCommunities } from "@/features/communities/useCommunities";
import type { ProjectContainer } from "@/features/projects-container/hooks";
import { useProjectRosterQuery } from "@/features/projects-container/lib/projectMembers";
import { relayClient } from "@/shared/api/relayClient";
import { KIND_MANAGED_AGENT } from "@/shared/constants/kinds";

import {
  acceptPublishedProjectAgents,
  PUBLISHED_PROJECT_AGENTS_READ_LIMIT,
  type PublishedProjectAgent,
  projectAgentAuthorizedAuthors,
  readsPublishedProjectAgents,
} from "./publishedProjectAgents";

const NO_AGENTS: readonly PublishedProjectAgent[] = [];

function errorSentence(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Relay-scoped key. The QueryClient outlives a community switch, and a
 * project coordinate names no relay, so the relay URL is part of the key
 * rather than a reset hook.
 */
export function publishedProjectAgentsQueryKey(
  relayUrl: string | null,
  projectRef: string | null,
  authors: readonly string[],
  rosterVerified: boolean,
) {
  return [
    "project-published-agents",
    relayUrl ?? "",
    projectRef ?? "",
    authors.join(","),
    rosterVerified ? "verified" : "unverified",
  ] as const;
}

export type PublishedProjectAgentsState = {
  /**
   * Accepted associations naming this project, from authorized authors, each
   * with whether its author's authority was verified. Always empty for a
   * private project, which is never read.
   */
  agents: readonly PublishedProjectAgent[];
  /** The project is private: nothing is published for it, nothing was read. */
  isPrivate: boolean;
  /** The authors whose claims were read: creator, roster owners, collaborators. */
  authorizedAuthors: readonly string[];
  isLoading: boolean;
  /** Each read limit or failure, in its own words. */
  notices: string[];
  /** The project's roster as read (for access decisions), or `[]`. */
  roster: NonNullable<ReturnType<typeof useProjectRosterQuery>["data"]>;
  rosterLoading: boolean;
  rosterError: string | null;
};

/**
 * Agents other computers associated with this project: kind:30177 by the
 * project's creator and roster owners/collaborators, newest per owner and
 * agent, matching this project's digest. One coalesced `POST /query`.
 */
export function usePublishedProjectAgents(
  project: ProjectContainer | null,
): PublishedProjectAgentsState {
  const { activeCommunity } = useCommunities();
  const relayUrl = activeCommunity?.relayUrl ?? null;
  const rosterQuery = useProjectRosterQuery(project);
  // A project with no owner (the local placeholder) has no roster to wait for.
  const rosterReady =
    !project?.owner || rosterQuery.isSuccess || rosterQuery.isError;
  const roster = React.useMemo(
    () => rosterQuery.data ?? project?.members ?? [],
    [project?.members, rosterQuery.data],
  );
  const authorizedAuthors = React.useMemo(
    () => projectAgentAuthorizedAuthors(project?.owner ?? null, roster),
    [project?.owner, roster],
  );
  const projectRef = project?.address ?? null;
  // Only a roster that was actually read verifies a collaborator's authority;
  // the head's member list after a failed read is a fallback, not a proof.
  const rosterVerified = rosterQuery.isSuccess;
  const reads = readsPublishedProjectAgents(project);

  const query = useQuery({
    enabled:
      reads &&
      projectRef !== null &&
      relayUrl !== null &&
      rosterReady &&
      authorizedAuthors.length > 0,
    queryKey: publishedProjectAgentsQueryKey(
      relayUrl,
      projectRef,
      authorizedAuthors,
      rosterVerified,
    ),
    queryFn: async () => {
      const events = await relayClient.fetchEventsCoalesced({
        kinds: [KIND_MANAGED_AGENT],
        authors: [...authorizedAuthors],
        limit: PUBLISHED_PROJECT_AGENTS_READ_LIMIT,
      });
      return {
        read: events.length,
        agents: acceptPublishedProjectAgents({
          events,
          projectRef,
          authorizedAuthors,
          rosterVerified,
        }),
      };
    },
    staleTime: 30_000,
  });

  const rosterError = rosterQuery.isError
    ? errorSentence(rosterQuery.error)
    : null;
  const notices = React.useMemo(() => {
    const list: string[] = [];
    if (rosterError && reads) {
      list.push(
        `Project members could not be read, so agents published by anyone other than its creator are listed as project authority not verified: ${rosterError}`,
      );
    }
    if (query.isError) {
      list.push(
        `Agents associated on other computers could not be read: ${errorSentence(query.error)}`,
      );
    }
    if ((query.data?.read ?? 0) >= PUBLISHED_PROJECT_AGENTS_READ_LIMIT) {
      list.push(
        `Only the newest ${PUBLISHED_PROJECT_AGENTS_READ_LIMIT} agent records from this project's owners and collaborators were read; agents on other computers may be missing.`,
      );
    }
    return list;
  }, [query.data?.read, query.error, query.isError, reads, rosterError]);

  return {
    agents: reads ? (query.data?.agents ?? NO_AGENTS) : NO_AGENTS,
    isPrivate: project !== null && !reads,
    authorizedAuthors,
    isLoading: !rosterReady || (reads && query.isLoading),
    notices,
    roster,
    rosterLoading: !rosterReady,
    rosterError,
  };
}
