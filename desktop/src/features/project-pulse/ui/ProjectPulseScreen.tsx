import * as React from "react";
import { useQueryClient } from "@tanstack/react-query";

import { useAppNavigation } from "@/app/navigation/useAppNavigation";
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

import { listCodingSessionSeatWorktrees } from "@/shared/api/tauriCodingSessionWorktrees";

import { projectPulseChannelIds } from "../lib/pulseChannelSet";
import { projectPulseDeclaredWork } from "../lib/pulseDeclaredWork";
import { buildPulseDiskRow, type PulseDiskRow } from "../lib/pulseDiskRow";
import type { PulseDigestSession } from "../lib/pulseFold.ts";
import {
  projectPulseChannelSetUnresolved,
  projectPulseQueryKey,
  useProjectPulseDigest,
  usePulseDeclaredWork,
  usePulseMissionRows,
} from "../lib/pulseQueries";
import { pulseSessionRouteParams } from "../lib/pulseSessionRoute";
import {
  ProjectPulseView,
  type ProjectPulseViewState,
} from "./ProjectPulseView";
import { usePulseAuthorNames } from "./usePulseAuthorNames";

const NO_PULSE_SESSIONS: readonly PulseDigestSession[] = [];

/**
 * This project's disk row (L11), scoped to the sessions this project's own
 * digest already named.
 *
 * Deliberately **not** `bee sessions worktree status --all`'s machine-wide
 * scope: the host command answers about whatever session refs it is asked
 * about (`worktree_prune.rs`'s `list_coding_session_seat_worktrees` — "every
 * recorded worktree for the sessions the caller named"), so handing it this
 * project's own sessions gives an honest per-project slice rather than a
 * global claim from a project screen that never asked a global question. A
 * result with nothing to show — no sessions, no rows, or the model's own
 * "no worktrees recorded" branch — renders no row at all, exactly like a
 * host that cannot answer (`useSeatWorktrees` in
 * `useCodingSessionClosureDialog.tsx` is the precedent).
 *
 * Neither `tipOnRelay` nor `settledForSecs` is knowable from this screen — it
 * does not independently establish relay ref state or measure settle
 * duration — so both are `null`, never guessed.
 */
function useProjectPulseDiskRow(
  sessions: readonly PulseDigestSession[],
): PulseDiskRow | undefined {
  const [row, setRow] = React.useState<PulseDiskRow | undefined>(undefined);

  React.useEffect(() => {
    const facts = sessions
      .filter(
        (session): session is PulseDigestSession & { sessionRef: string } =>
          Boolean(session.sessionRef),
      )
      .map((session) => ({
        sessionRef: session.sessionRef,
        sessionSettled: session.lifecycle === "closed",
        executionLive: session.coordinationState === "provider_reachable",
        tipOnRelay: null,
        settledForSecs: null,
      }));
    if (facts.length === 0) {
      setRow(undefined);
      return;
    }
    let cancelled = false;
    void listCodingSessionSeatWorktrees(facts)
      .then((rows) => {
        if (cancelled) return;
        const built = buildPulseDiskRow(rows);
        setRow(built.empty ? undefined : built);
      })
      .catch(() => {
        if (!cancelled) setRow(undefined);
      });
    return () => {
      cancelled = true;
    };
  }, [sessions]);

  return row;
}

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

  // The sibling mission read. Same coordinate, same channel floor, its own
  // failure mode: a mission read that could not be decoded renders as a
  // disclosed failure on the view below, never as a project with no missions.
  //
  // It is handed **this paint's** digest rather than the last cached one, so
  // the sessions it opens are the sessions the screen is showing; without that
  // it read one digest behind and, before the sessions were wired at all,
  // asserted a scope it had never opened (REVIEW-L9 F1).
  //
  // The names are the digest's own author map. It resolves entry authors, so a
  // seat that has never written a Pulse entry still renders as 8 hex — a
  // weaker rendering of the same fact, never a wrong one. A batch profile read
  // over the seat pubkeys the response returns would close it.
  const missionNames = React.useMemo(
    () => Object.fromEntries(authorNames),
    [authorNames],
  );
  const missions = usePulseMissionRows(coordinate, channelIds, {
    digest: pulse.digest,
    displayNames: missionNames,
  });

  // This project's disk row (L11 x L9 join, gap closed in L17): scoped to
  // exactly the sessions this paint's digest named, never the whole machine.
  const diskRow = useProjectPulseDiskRow(
    pulse.digest?.sessions ?? NO_PULSE_SESSIONS,
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

  // The declared-work read: plans regrouped out of the entries list, joined to
  // the assignments the native projection folded over the visible sessions.
  // Same coordinate, same channel floor, this paint's digest — and its own
  // failure mode, disclosed on the section rather than rendered as "no work".
  const declaredRead = usePulseDeclaredWork(coordinate, channelIds, {
    digest: pulse.digest,
  });
  const declaredPages = declaredRead.pages;
  // Destructured so the memoised props below depend on the stable functions
  // rather than the state object, which is a new object every render.
  const declaredFetchNextPage = declaredRead.fetchNextPage;
  const declaredRefetch = declaredRead.refetch;
  const queryClient = useQueryClient();
  const { goCodingSession } = useAppNavigation();
  const declaredModel = React.useMemo(
    () =>
      projectPulseDeclaredWork({
        digest: pulse.digest ?? null,
        pages: declaredPages,
        pageErrors: declaredRead.pageErrors,
        visibleSessionCount: declaredRead.visibleSessionCount,
        loadedPageCount: declaredRead.loadedPageCount,
        nowSeconds,
        // The viewer the *response* was computed for, not one this screen
        // reads independently: a mismatch would silently re-scope the model.
        viewerPubkey: declaredPages[0]?.response.viewerPubkey ?? null,
      }),
    [
      declaredPages,
      declaredRead.loadedPageCount,
      declaredRead.pageErrors,
      declaredRead.visibleSessionCount,
      nowSeconds,
      pulse.digest,
    ],
  );

  // Which channel each declared-work session was read from. Only the response
  // knows: the digest session carries no channel, and guessing one from the
  // project's floor would open the wrong channel's transcript.
  const declaredChannelBySession = React.useMemo(() => {
    const byKey = new Map<string, string>();
    for (const page of declaredPages) {
      for (const session of page.response.sessions) {
        byKey.set(session.sessionKey, session.channelId);
      }
    }
    return byKey;
  }, [declaredPages]);
  const digestSessionsByKey = React.useMemo(() => {
    const byKey = new Map<string, PulseDigestSession>();
    for (const session of pulse.digest?.sessions ?? []) {
      byKey.set(session.sessionKey, session);
    }
    return byKey;
  }, [pulse.digest]);
  // The route this session's "Open session" would take, or null when no
  // execution is recorded for it. One resolver, so the control and the
  // sentence that replaces it can never disagree.
  const declaredRoute = React.useCallback(
    (sessionKey: string) => {
      const channelId = declaredChannelBySession.get(sessionKey);
      const session = digestSessionsByKey.get(sessionKey);
      if (!channelId || !session) return null;
      return pulseSessionRouteParams({ channelId, session });
    },
    [declaredChannelBySession, digestSessionsByKey],
  );
  const declaredSessionOpenable = React.useCallback(
    (sessionKey: string) => declaredRoute(sessionKey) !== null,
    [declaredRoute],
  );
  // Navigation only: it opens a route. It publishes no event, sends no turn,
  // and starts nothing — reading a source is not scheduling an agent.
  const openDeclaredSession = React.useCallback(
    (sessionKey: string) => {
      const params = declaredRoute(sessionKey);
      if (!params) return;
      goCodingSession(params.channelId, params.generationId);
    },
    [declaredRoute, goCodingSession],
  );
  // "Check again" re-reads what is already loaded: this hook's pages, and the
  // digest they are a sibling of. It starts no model and no agent.
  const recheckDeclaredWork = React.useCallback(() => {
    declaredRefetch();
    if (coordinate === null) return;
    void queryClient.invalidateQueries({
      queryKey: projectPulseQueryKey(coordinate, channelIds, unresolved),
    });
  }, [channelIds, coordinate, declaredRefetch, queryClient, unresolved]);
  const declaredWork = React.useMemo(
    () => ({
      model: declaredModel,
      state: declaredRead.kind,
      message: declaredRead.message,
      hasNextPage: declaredRead.hasNextPage,
      isFetchingNextPage: declaredRead.isFetchingNextPage,
      onLoadMore: declaredFetchNextPage,
      onRecheck: recheckDeclaredWork,
      refreshing: declaredRead.refreshing,
    }),
    [
      declaredFetchNextPage,
      declaredModel,
      declaredRead.hasNextPage,
      declaredRead.isFetchingNextPage,
      declaredRead.kind,
      declaredRead.message,
      declaredRead.refreshing,
      recheckDeclaredWork,
    ],
  );

  return (
    <ProjectPulseView
      authorNames={authorNames}
      declaredSessionOpenable={declaredSessionOpenable}
      declaredWork={declaredWork}
      diskRow={diskRow}
      nowSeconds={nowSeconds}
      onOpenDeclaredSession={openDeclaredSession}
      // Named, not implied by a 300px-away sidebar selection: two projects'
      // Pulse screens are otherwise pixel-identical chrome, and "No Pulse yet"
      // read against the wrong project is a coordination lie. The heading
      // carries that name; the way back is the window's own back control.
      missions={missions}
      projectName={embedded ? null : (project?.name ?? null)}
      state={state}
    />
  );
}
