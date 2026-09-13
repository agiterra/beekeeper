import * as React from "react";

import { useChannelsQuery } from "@/features/channels/hooks";
import { useProjectContainersQuery } from "@/features/projects-container/hooks";
import { useProjectRosterQuery } from "@/features/projects-container/lib/projectMembers";
import { useIdentityQuery } from "@/shared/api/hooks";

import {
  type CodingSessionAccessReadStatus,
  type CodingSessionChannelAccess,
  PROJECT_LIST_FETCH_LIMIT,
  projectCreatorFromAddress,
  resolveCodingSessionChannelAccess,
} from "../lib/codingSessionChannelAccess";

/**
 * The signed-in identity's write access to a session's channel, with every
 * read it depends on: the channel list (transports included), the identity,
 * the project heads and the transport's project roster.
 *
 * The project list is read only for a transport the identity is not a member
 * of, and the roster only when the project exists and the identity did not
 * create it — so an ordinary channel never waits on either.
 *
 * The roster is the cached kind:39010 read (30 s stale time); no live
 * subscription refreshes it here, and the relay re-decides every write.
 */
export function useCodingSessionChannelAccess(
  channelId: string,
): CodingSessionChannelAccess {
  const identity = useIdentityQuery();
  const currentUserPubkey = identity.data?.pubkey ?? null;
  const channelsQuery = useChannelsQuery({
    enabled: true,
    includeSessionTransports: true,
  });
  const channel =
    channelsQuery.data?.find((candidate) => candidate.id === channelId) ?? null;
  const channelsStatus: CodingSessionAccessReadStatus =
    channelsQuery.data === undefined
      ? channelsQuery.isError
        ? "error"
        : "loading"
      : // A channel missing from a list that is still refreshing may be one
        // the refresh is about to bring; only a settled list says "absent".
        channel === null && channelsQuery.isFetching
        ? "loading"
        : "ready";

  const projectRef = channel?.projectRef ?? null;
  const creator = projectCreatorFromAddress(projectRef);
  const self = currentUserPubkey?.toLowerCase() ?? null;
  const needsProject =
    channel !== null &&
    !channel.isMember &&
    channel.channelType === "transport" &&
    creator !== null &&
    self !== null;

  const containers = useProjectContainersQuery({ enabled: needsProject });
  const project = React.useMemo(
    () =>
      needsProject && projectRef
        ? // Exact, as the relay joins `project_acl.coordinate = project_ref`.
          (containers.data?.find(
            (candidate) => candidate.address === projectRef,
          ) ?? null)
        : null,
    [containers.data, needsProject, projectRef],
  );
  const projectStatus: CodingSessionAccessReadStatus = !needsProject
    ? "ready"
    : containers.data === undefined
      ? containers.isError
        ? "error"
        : "loading"
      : project === null &&
          (containers.isPlaceholderData || containers.isFetching)
        ? "loading"
        : "ready";
  const truncated =
    (containers.data ?? []).filter((candidate) => candidate.owner.length > 0)
      .length >= PROJECT_LIST_FETCH_LIMIT;

  const needsRoster = project !== null && self !== creator;
  const rosterQuery = useProjectRosterQuery(needsRoster ? project : null);
  const rosterStatus: CodingSessionAccessReadStatus = !needsRoster
    ? "ready"
    : rosterQuery.data !== undefined
      ? "ready"
      : rosterQuery.isError
        ? "error"
        : "loading";

  return resolveCodingSessionChannelAccess({
    channel,
    channelsStatus,
    currentUserPubkey,
    project: {
      status: projectStatus,
      found: project !== null,
      truncated,
    },
    roster: { status: rosterStatus, members: rosterQuery.data ?? [] },
  });
}
