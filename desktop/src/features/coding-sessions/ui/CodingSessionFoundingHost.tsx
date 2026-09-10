import * as React from "react";
import { toast } from "sonner";

import { useAppNavigation } from "@/app/navigation/useAppNavigation";
import { writeCodingSessionFoundedDraft } from "../lib/codingSessionFoundedDraft";
import {
  type NewCodingSessionWorkspaceReuse,
  workspaceReuseRepoRef,
} from "../lib/codingSessionWorkspaceReuse";
import {
  clearCodingSessionFoundingRequest,
  type CodingSessionFoundingRequest,
  markCodingSessionFoundingStarted,
  useCodingSessionFoundingRequest,
} from "../newCodingSessionDialogStore";
import {
  type CodingSessionTopicFoundingHostDeps,
  useCodingSessionTopicFounding,
} from "./useCodingSessionTopicFounding";

/**
 * Said when a request names no channel and no project: the one entry with no
 * destination (`/coding-sessions/new` with no `channelId`) cannot found
 * anything without guessing a channel, so it does not.
 */
export const CODING_SESSION_FOUNDING_NO_DESTINATION_SENTENCE =
  "Pick a channel or a project to start a coding session in.";

/** Where the founded session's page is. Injected so a test needs no router. */
export type GoFoundedCodingSession = (
  channelId: string,
  sessionRef: string,
) => unknown;

/** What one founding knows at the click: destination, repository, workspace. */
export type CodingSessionFoundNowInput = {
  /** The channel, or null when `ensureChannelId` will mint it. */
  channelId: string | null;
  /** Kept on the result; nothing at founding signs it. */
  projectRef: string | null;
  /** LANE-L20: resolved at the click, or null — never guessed. */
  repoRef: string | null;
  /** The reused checkout, or null for an ordinary founding. */
  workspace: NewCodingSessionWorkspaceReuse | null;
  /** The workdir prefill when no workspace is reused (a project checkout). */
  defaultWorkdir: string | null;
};

/**
 * The project flow pulls in the whole projects-container feature — its
 * containers query, its channel partitioning, its repo-checkout resolution.
 * None of that should load for someone who never founds a project session,
 * so it arrives only when a project request does.
 */
const ProjectCodingSessionFounder = React.lazy(async () => {
  const module = await import(
    "@/features/projects-container/ui/ProjectCodingSessionFounder"
  );
  return { default: module.ProjectCodingSessionFounder };
});

/**
 * Found the session now, and land on its page.
 *
 * The whole click, in order: claim the request (synchronously — this is what
 * makes a StrictMode double effect, a remount or a second click found exactly
 * once), a loading toast, the founding (one 44226; the goal and the name are
 * published from the page as the founder commits them), the founded draft v2
 * (what the click knew about where it runs), clear the request, navigate. A
 * failure is a `toast.error` in the publisher's own words, and the request is
 * cleared either way — a request left standing would block every later click.
 *
 * The navigation goes through a ref, and nothing here cancels an in-flight
 * founding: an effect cleanup that abandoned the promise would leave a
 * genesis on the wire with nobody landing on it.
 */
export function useCodingSessionFoundNow(input: {
  /** The project flow's own channel minting; null for a known channel. */
  ensureChannelId?: (() => Promise<string>) | null;
  goFoundedCodingSession: GoFoundedCodingSession;
  /** Injected in tests; this computer's relay and keyring by default. */
  deps?: CodingSessionTopicFoundingHostDeps;
}): { foundNow: (target: CodingSessionFoundNowInput) => Promise<void> } {
  const { found } = useCodingSessionTopicFounding({
    ensureChannelId: input.ensureChannelId ?? null,
    deps: input.deps,
  });
  const navigateRef = React.useRef(input.goFoundedCodingSession);
  navigateRef.current = input.goFoundedCodingSession;

  const foundNow = React.useCallback(
    async (target: CodingSessionFoundNowInput) => {
      // Before the first await: the second of two same-tick callers gets
      // false here and does nothing.
      if (!markCodingSessionFoundingStarted()) return;
      const toastId = toast.loading("Founding a session…");
      try {
        const founded = await found({
          channelId: target.channelId,
          goal: "",
          title: null,
          projectRef: target.projectRef,
          repoRef: target.repoRef,
        });
        if (
          !founded.ok ||
          founded.channelId === null ||
          founded.sessionRef === null
        ) {
          clearCodingSessionFoundingRequest();
          toast.error(
            founded.failureReason ?? "The session could not be founded.",
          );
          return;
        }
        const workspace = target.workspace;
        writeCodingSessionFoundedDraft(founded.sessionRef, {
          name: null,
          workdir: workspace?.path ?? target.defaultWorkdir ?? null,
          useWorktree: workspace === null,
          worktreeName: null,
          worktreeSource: null,
          rememberWorkspace: workspace === null,
          repoRef: target.repoRef,
          workspaceSourcePath: workspace?.path ?? null,
          workspaceSourceBranch: workspace?.branch ?? null,
          workspaceSourceBranchSource:
            workspace?.branchSource === "recorded" ? "recorded" : null,
        });
        clearCodingSessionFoundingRequest();
        // Push, not replace: Back returns to where the click was.
        void navigateRef.current(founded.channelId, founded.sessionRef);
      } catch (error) {
        // `found` resolves rather than rejecting; this is the belt over that
        // brace, so an unexpected throw never leaves the request standing.
        clearCodingSessionFoundingRequest();
        toast.error(
          error instanceof Error && error.message.trim().length > 0
            ? error.message
            : "The session could not be founded.",
        );
      } finally {
        toast.dismiss(toastId);
      }
    },
    [found],
  );

  return { foundNow };
}

/**
 * How a request is founded: right here, through the project founder, or not
 * at all.
 *
 * A workspace request that names a project but no channel goes through the
 * project founder, which resolves the project's sessions channel (minting it
 * if the project has never had one). One that names a channel is founded
 * right here: the reused checkout answers where the session *runs*, its
 * `repoRef` is the source session's (never a project default), and nothing
 * at founding signs a project coordinate — the founded page reads placement
 * from the channel.
 */
export function resolveCodingSessionFoundingRoute(
  request: CodingSessionFoundingRequest,
):
  | {
      kind: "project";
      projectId: string;
      sourceRepoRef: string | null;
      workspaceReuse: NewCodingSessionWorkspaceReuse | null;
    }
  | { kind: "channel"; target: CodingSessionFoundNowInput }
  | { kind: "nowhere" } {
  if (request.kind === "project") {
    return {
      kind: "project",
      projectId: request.projectId,
      sourceRepoRef: null,
      workspaceReuse: null,
    };
  }
  if (request.kind === "workspace") {
    if (request.channelId !== null && request.channelId.length > 0) {
      return {
        kind: "channel",
        target: {
          channelId: request.channelId,
          projectRef: null,
          repoRef: workspaceReuseRepoRef({
            contextual: true,
            sourceRepoRef: request.sourceRepoRef ?? null,
            projectRepoRef: null,
          }),
          workspace: request.workspace,
          defaultWorkdir: null,
        },
      };
    }
    if (request.projectId !== null && request.projectId.length > 0) {
      return {
        kind: "project",
        projectId: request.projectId,
        sourceRepoRef: request.sourceRepoRef ?? null,
        workspaceReuse: request.workspace,
      };
    }
    return { kind: "nowhere" };
  }
  if (request.channelId !== null && request.channelId.length > 0) {
    return {
      kind: "channel",
      target: {
        channelId: request.channelId,
        projectRef: null,
        repoRef: null,
        workspace: null,
        defaultWorkdir: null,
      },
    };
  }
  return { kind: "nowhere" };
}

function ChannelFounding({
  deps,
  goFoundedCodingSession,
  target,
}: {
  deps?: CodingSessionTopicFoundingHostDeps;
  goFoundedCodingSession: GoFoundedCodingSession;
  target: CodingSessionFoundNowInput;
}) {
  const { foundNow } = useCodingSessionFoundNow({
    goFoundedCodingSession,
    deps,
  });
  React.useEffect(() => {
    void foundNow(target);
    // No cleanup on purpose: see `useCodingSessionFoundNow`.
  }, [foundNow, target]);
  return null;
}

function NoDestination() {
  React.useEffect(() => {
    // The same claim as a founding, so the sentence is said once even when
    // StrictMode runs this effect twice.
    if (!markCodingSessionFoundingStarted()) return;
    clearCodingSessionFoundingRequest();
    toast.info(CODING_SESSION_FOUNDING_NO_DESTINATION_SENTENCE);
  }, []);
  return null;
}

/**
 * The founding host with its navigation injected: what the tests mount, and
 * what {@link CodingSessionFoundingHost} mounts with the app's router.
 */
export function CodingSessionFoundingRunner({
  deps,
  goFoundedCodingSession,
}: {
  /** Injected in tests; this computer's relay and keyring by default. */
  deps?: CodingSessionTopicFoundingHostDeps;
  goFoundedCodingSession: GoFoundedCodingSession;
}) {
  const request = useCodingSessionFoundingRequest();
  const route = React.useMemo(
    () =>
      request === null ? null : resolveCodingSessionFoundingRoute(request),
    [request],
  );
  if (route === null) return null;
  if (route.kind === "nowhere") return <NoDestination />;
  if (route.kind === "project") {
    return (
      // No fallback: there is nothing to show while the chunk resolves — the
      // loading toast is raised by the founder itself once it can found.
      <React.Suspense fallback={null}>
        <ProjectCodingSessionFounder
          deps={deps}
          goFoundedCodingSession={goFoundedCodingSession}
          projectId={route.projectId}
          sourceRepoRef={route.sourceRepoRef}
          workspaceReuse={route.workspaceReuse}
        />
      </React.Suspense>
    );
  }
  return (
    <ChannelFounding
      deps={deps}
      goFoundedCodingSession={goFoundedCodingSession}
      target={route.target}
    />
  );
}

/**
 * The one place a "New coding session" click is performed.
 *
 * Mounted by the app shell rather than by each caller: a session can be
 * founded from a channel menu, a project sidebar, a session header or a deep
 * link, and several hosts would be several chances to found one click twice.
 * It renders nothing; the click's only visible trace is a loading toast, and
 * then the founded session's page.
 */
export function CodingSessionFoundingHost() {
  const { goFoundedCodingSession } = useAppNavigation();
  return (
    <CodingSessionFoundingRunner
      goFoundedCodingSession={goFoundedCodingSession}
    />
  );
}
