import * as React from "react";

import { useChannelsQuery } from "@/features/channels/hooks";
import {
  partitionChannels,
  useProjectContainers,
  useProjectContainersQuery,
} from "@/features/projects-container/hooks";
import {
  GENERAL_PROJECT_DTAG,
  LOCAL_GENERAL_ID,
  makeLocalGeneral,
} from "@/features/projects-container/lib/projectContainerModel";
import type { ProjectContainer } from "@/features/projects-container/lib/projectContainerModel";

import { projectPulseChannelIds } from "../lib/pulseChannelSet";
import {
  projectPulseChannelSetUnresolved,
  useProjectPulseDigest,
} from "../lib/pulseQueries";
import {
  ProjectPulseView,
  type ProjectPulseViewState,
} from "./ProjectPulseView";
import { usePulseAuthorNames } from "./usePulseAuthorNames";

/** A project's channel set, plus whether that set is a settled answer. */
export type ProjectPulseChannelSet = {
  channelIds: string[];
  /**
   * True while the channel list has not resolved (pending) or failed to
   * resolve (error). The set is then a floor, not the project's channels, and
   * a session read against it must be reported as partial — an unresolved set
   * that silently produced zero channels is the "quiet project" lie.
   */
  unresolved: boolean;
};

/**
 * Resolve the project's channel set — the only place session facts are read
 * from. The set itself is built by {@link projectPulseChannelIds}, which
 * mirrors `channels.project_ref` including transport channels; this hook only
 * supplies its inputs and reports whether the channel list resolved at all.
 */
export function useProjectPulseChannelIds(
  project: ProjectContainer | null,
): ProjectPulseChannelSet {
  const channelsQuery = useChannelsQuery({ includeSessionTransports: true });
  const { projects } = useProjectContainers();
  // `initialDataUpdatedAt: 0` makes a persisted channel snapshot immediately
  // visible but explicitly stale. Until its authoritative hash revalidation
  // settles, that list is a floor and Pulse must remain partial.
  const unresolved = projectPulseChannelSetUnresolved(channelsQuery);
  return React.useMemo(() => {
    if (!project) return { channelIds: [], unresolved };
    const channels = channelsQuery.data ?? [];
    const buckets = partitionChannels(projects, channels);
    const bucketed = [
      ...(buckets.channelsByProject.get(project.id) ?? []),
      ...(buckets.forumsByProject.get(project.id) ?? []),
    ].map((channel) => channel.id);
    return {
      channelIds: projectPulseChannelIds(project, channels, bucketed),
      unresolved,
    };
  }, [channelsQuery.data, project, projects, unresolved]);
}

/**
 * The project this route names, or null when no readable head matches it.
 *
 * `isLoading` is returned alongside because "no head matched" and "the heads
 * have not arrived yet" are different answers: on a cold start or right after
 * a community switch `projects` is `[]` while the containers query is still in
 * flight, and rendering that as "this project's head is not readable" would
 * state a completed negative verdict about a read still in progress.
 *
 * It reads the containers query directly rather than `useProjectContainers()`'s
 * `isLoading`, which is the wrong signal here: `useProjectContainersQuery`
 * supplies `placeholderData` from a localStorage snapshot, so React Query
 * reports `success` (and `isLoading` false) the instant a stale snapshot
 * exists, while the authoritative kind:30621 read is still in flight. A deep
 * link to a project missing from that snapshot — just created, or created on
 * another device — would otherwise render the completed "head is not readable"
 * verdict for a perfectly readable project. Only a settled, non-placeholder
 * result that still lacks the coordinate is a real negative.
 */
export function useProjectPulseProject(projectId: string): {
  project: ProjectContainer | null;
  isLoading: boolean;
} {
  const containersQuery = useProjectContainersQuery();
  const projects = React.useMemo(
    () => containersQuery.data ?? [],
    [containersQuery.data],
  );
  const isLoading =
    containersQuery.isPending ||
    containersQuery.isPlaceholderData ||
    containersQuery.isFetching;
  const project = React.useMemo(() => {
    if (projectId === LOCAL_GENERAL_ID) {
      return (
        projects.find((candidate) => candidate.dtag === GENERAL_PROJECT_DTAG) ??
        makeLocalGeneral()
      );
    }
    return (
      projects.find(
        (candidate) =>
          candidate.id === projectId || candidate.dtag === projectId,
      ) ?? null
    );
  }, [projects, projectId]);
  return { project, isLoading };
}

/**
 * `/projects/$projectId/pulse` — one project's current coordination view.
 *
 * The screen answers with facts or says it could not: a read that fails
 * renders as a partial read, never as a quiet project, and a project whose
 * head this community cannot read renders as unavailable rather than empty.
 */
export function ProjectPulseScreen({
  projectId,
  embedded = false,
}: {
  projectId: string;
  /**
   * True when the project page hosts the screen as a tab: the page already
   * names the project and owns navigation, so the back affordance and the
   * project-name header are omitted rather than drawn twice.
   */
  embedded?: boolean;
}) {
  const { project, isLoading: projectLoading } =
    useProjectPulseProject(projectId);
  const { channelIds, unresolved } = useProjectPulseChannelIds(project);
  // The local General placeholder has no coordinate; a Pulse query for it
  // could never match, so the hook stays disabled and the screen says so.
  const isFallback = project?.id === LOCAL_GENERAL_ID;
  const coordinate = project && !isFallback ? project.address : null;
  const pulse = useProjectPulseDigest(coordinate, channelIds, unresolved);

  const state: ProjectPulseViewState = React.useMemo(() => {
    // The local General placeholder is a settled answer — it has no
    // coordinate and never will, so it is unavailable immediately.
    if (isFallback) return { kind: "unavailable" };
    // No head matched *yet* is not the same claim as no head exists. While the
    // containers query is in flight (cold start, or right after a community
    // switch, where `projects` is `[]` until it resolves) this is a read in
    // progress, not the completed negative verdict "this project's head is not
    // readable from this community."
    if (!project) {
      return projectLoading ? { kind: "loading" } : { kind: "unavailable" };
    }
    if (pulse.kind === "loading") {
      // A cached digest is the *last complete read*, not the current one. It
      // paints (a screen that blanks on every refetch is worse), but it is
      // marked so the header can say so — an entry posted since that read is
      // simply absent, and nothing else on the screen would admit it.
      return pulse.digest
        ? { kind: "ready", digest: pulse.digest, refreshing: true }
        : { kind: "loading" };
    }
    return pulse;
  }, [isFallback, project, projectLoading, pulse]);

  const authorNames = usePulseAuthorNames(
    state.kind === "ready" || state.kind === "partial" ? state.digest : null,
  );

  // One clock for the whole paint, ticking once a minute so ages stay honest
  // without re-rendering on every frame.
  const [nowSeconds, setNowSeconds] = React.useState(() =>
    Math.floor(Date.now() / 1_000),
  );
  React.useEffect(() => {
    const timer = window.setInterval(
      () => setNowSeconds(Math.floor(Date.now() / 1_000)),
      60_000,
    );
    return () => window.clearInterval(timer);
  }, []);

  return (
    <ProjectPulseView
      authorNames={authorNames}
      nowSeconds={nowSeconds}
      // Named, not implied by a 300px-away sidebar selection: two projects'
      // Pulse screens are otherwise pixel-identical chrome, and "No Pulse yet"
      // read against the wrong project is a coordination lie. The heading
      // carries that name; the way back is the window's own back control.
      projectName={embedded ? null : (project?.name ?? null)}
      state={state}
    />
  );
}
