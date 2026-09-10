import { CircleAlert } from "lucide-react";
import * as React from "react";
import type { ReactNode } from "react";

import { useAppNavigation } from "@/app/navigation/useAppNavigation";
import { useChannelsQuery } from "@/features/channels/hooks";
import { useIdentityQuery } from "@/shared/api/hooks";
import { Button } from "@/shared/ui/button";
import { FuzzyLogo } from "@/shared/ui/buzz-logo/FuzzyLogo";
import {
  codingSessionClosureIsClosed,
  codingSessionClosureKey,
} from "../../lib/codingSessionClosure";
import {
  type CodingSessionFoundedRouteResolution,
  type CodingSessionFoundedUmbrella,
  founderPubkeysByGenesisRef,
  resolveFoundedCodingSessionRoute,
} from "../../lib/codingSessionFoundedModel";
import {
  CODING_SESSION_GOAL_UNRESOLVED,
  codingSessionGoalErrorSentence,
  deriveCodingSessionGoalReader,
} from "../../lib/codingSessionGoal";
import { selectCodingSessionUmbrellaGoal } from "../../lib/codingSessionMissionGoal";
import { codingSessionNameKey } from "../../lib/codingSessionName";
import { useCodingSessionCatalog } from "../../useCodingSessionCatalog";
import { useCodingSessionClosures } from "../../useCodingSessionClosures";
import { useCodingSessionGoals } from "../../useCodingSessionGoals";
import { useCodingSessionNames } from "../../useCodingSessionNames";
import { CodingSessionFounderLine } from "../CodingSessionFounderLine";
import { CodingSessionHeader } from "../CodingSessionHeader";
import {
  CodingSessionFoundedSetupHost,
  CodingSessionFoundedSetupReadOnly,
} from "./CodingSessionFoundedSetupCard";

/**
 * The umbrella's goal as this screen could read it. `text` is the wire goal;
 * every other kind is said, and the text hook publishes nothing over it.
 */
export type CodingSessionFoundedGoal =
  | { kind: "available"; text: string }
  | { kind: "unresolved" }
  | { kind: "errored"; message: string }
  | { kind: "absent" }
  | { kind: "rejected" };

/** What the goal line says, when there is no goal to say. */
export function codingSessionFoundedGoalSentence(
  goal: CodingSessionFoundedGoal,
): string | null {
  switch (goal.kind) {
    case "available":
      return null;
    case "unresolved":
      return CODING_SESSION_GOAL_UNRESOLVED;
    case "errored":
      return codingSessionGoalErrorSentence(goal.message);
    case "absent":
      return "No initial prompt for this session is on the relay yet.";
    case "rejected":
      return "A goal is on the relay under this session's ref, but not signed by its founder, so it is not this session's and Start will not send it.";
  }
}

/**
 * The route resolution with closures layered on top: `started` still wins
 * (a generation exists), then `closed` (an accepted 44230 `closed` for this
 * umbrella), then `starting` / `founded`. `resolveFoundedCodingSessionRoute`
 * itself does not read closures; this is the one place that combines them.
 */
export type CodingSessionFoundedViewResolution =
  | CodingSessionFoundedRouteResolution
  | { kind: "closed"; founded: CodingSessionFoundedUmbrella };

/** Layer the closure over the route resolution, by the precedence above. */
export function resolveFoundedCodingSessionView(input: {
  resolution: CodingSessionFoundedRouteResolution;
  closed: boolean;
}): CodingSessionFoundedViewResolution {
  const { resolution, closed } = input;
  if (
    closed &&
    (resolution.kind === "founded" || resolution.kind === "starting")
  ) {
    return { kind: "closed", founded: resolution.founded };
  }
  return resolution;
}

/**
 * A founded session: a genesis, and nothing running.
 *
 * Reads the founded projection for `(channelId, sessionRef)` and renders the
 * one screen the frozen workspace cannot: for the founder, the setup card
 * that names it, prompts it, and starts it (Solo or Team); for everybody
 * else, the name, the goal and who founded it. The moment the catalog reports
 * a generation under this umbrella the route replaces itself with the
 * generation route, once; a Start from this screen and a Start from another
 * member's desktop both land there the same way. A closure closes the page:
 * an abandoned click is a row on every device until it is discarded here.
 */
export function CodingSessionFoundedWorkspace({
  channelId,
  sessionRef,
}: {
  channelId: string;
  sessionRef: string;
}) {
  const identity = useIdentityQuery();
  const { goChannel, goCodingSession } = useAppNavigation();
  // Viewing is channel-membership authority, as the workspace reads it.
  const catalog = useCodingSessionCatalog(channelId, null, {
    authorityMode: "open",
  });
  const resolution = React.useMemo(
    () => resolveFoundedCodingSessionRoute({ catalog, channelId, sessionRef }),
    [catalog, channelId, sessionRef],
  );
  const channelsQuery = useChannelsQuery({
    enabled: true,
    includeSessionTransports: true,
  });
  const channel = React.useMemo(
    () => channelsQuery.data?.find((entry) => entry.id === channelId) ?? null,
    [channelId, channelsQuery.data],
  );
  // Until the channel list is read, `channel?.projectRef ?? null` says "no
  // project" about a channel nobody has looked at yet.
  const channelReader: "loading" | "errored" | "resolved" =
    channelsQuery.data !== undefined
      ? "resolved"
      : channelsQuery.isError
        ? "errored"
        : "loading";
  const channelIds = React.useMemo(() => [channelId], [channelId]);
  const goalSnapshot = useCodingSessionGoals(channelIds);
  const nameSnapshot = useCodingSessionNames(channelIds);
  const founders = React.useMemo(
    () => founderPubkeysByGenesisRef(catalog),
    [catalog],
  );
  const closures = useCodingSessionClosures(channelIds, founders);

  const founded =
    resolution.kind === "founded" || resolution.kind === "starting"
      ? resolution.founded
      : null;
  const founderPubkey = founded?.founderPubkey ?? null;
  const closure = founded
    ? (closures.closures.get(
        codingSessionClosureKey(channelId, sessionRef, founded.genesisRef),
      ) ?? null)
    : null;
  const viewResolution = React.useMemo(
    () =>
      resolveFoundedCodingSessionView({
        resolution,
        closed:
          closure !== null && codingSessionClosureIsClosed(closure.action),
      }),
    [closure, resolution],
  );
  const sessionName = founderPubkey
    ? (nameSnapshot.names.get(
        codingSessionNameKey(channelId, sessionRef, founderPubkey),
      )?.content ?? null)
    : null;
  const goal = React.useMemo<CodingSessionFoundedGoal>(() => {
    const selection = selectCodingSessionUmbrellaGoal({
      channelId,
      founderPubkey,
      goals: goalSnapshot.goals.values(),
      sessionRef,
    });
    if (selection.kind === "available") {
      return { kind: "available", text: selection.goal.content };
    }
    const reader = deriveCodingSessionGoalReader(goalSnapshot);
    if (reader.kind === "errored") {
      return { kind: "errored", message: reader.message };
    }
    if (reader.kind === "unresolved") return { kind: "unresolved" };
    return selection.kind === "rejected"
      ? { kind: "rejected" }
      : { kind: "absent" };
  }, [channelId, founderPubkey, goalSnapshot, sessionRef]);

  return (
    <CodingSessionFoundedView
      channelId={channelId}
      channelName={channel?.name ?? null}
      currentUserPubkey={identity.data?.pubkey ?? null}
      founderName={
        <CodingSessionFounderLine
          founderPubkey={founderPubkey}
          genesisRef={founded?.genesisRef ?? null}
          variant="label"
        />
      }
      goChannel={(target) => void goChannel(target)}
      goCodingSession={goCodingSession}
      goal={goal}
      resolution={viewResolution}
      sessionName={sessionName}
      sessionRef={sessionRef}
      setupCard={(umbrella, starting) => (
        <CodingSessionFoundedSetupHost
          channelId={channelId}
          channelReader={channelReader}
          founderPubkey={umbrella.founderPubkey}
          genesisRef={umbrella.genesisRef}
          goal={goal}
          nameResolved={nameSnapshot.resolved}
          onCreated={({ channelId: createdChannelId, generationId }) => {
            void goCodingSession(createdChannelId, generationId, {
              replace: true,
            });
          }}
          projectRef={channel?.projectRef ?? null}
          sessionRef={sessionRef}
          starting={starting}
          wireName={sessionName}
        />
      )}
    />
  );
}

/**
 * The founded screen from its facts. Split from the hooks above so a test can
 * hand it a resolution and read what it shows — and so the hand-off, the one
 * effect this screen owns, is pinned on the props that drive it.
 */
export function CodingSessionFoundedView({
  channelId,
  channelName,
  currentUserPubkey,
  founderName,
  goChannel,
  goCodingSession,
  goal,
  resolution,
  sessionName,
  sessionRef,
  setupCard,
}: {
  channelId: string;
  channelName: string | null;
  currentUserPubkey: string | null;
  /** The founder, named — a resolved profile label, or a plain string. */
  founderName: ReactNode;
  /** "Back to the channel" on the closed state. */
  goChannel: (channelId: string) => void;
  goCodingSession: (
    channelId: string,
    generationId: string,
    options: { replace: boolean },
  ) => Promise<unknown> | unknown;
  goal: CodingSessionFoundedGoal;
  resolution: CodingSessionFoundedViewResolution;
  sessionName: string | null;
  sessionRef: string;
  /** The founder's setup card; `starting` holds its Start (see the hook). */
  setupCard: (
    founded: CodingSessionFoundedUmbrella,
    starting: boolean,
  ) => ReactNode;
}) {
  // Once, whichever path made the generation appear: this screen's own
  // Start also navigates on its receipt, and `commitNavigation` drops a
  // navigation to the href already shown, so the two cannot double up.
  const navigatedRef = React.useRef(false);
  const generationId =
    resolution.kind === "started" ? resolution.generationId : null;
  React.useEffect(() => {
    if (generationId === null || navigatedRef.current) return;
    navigatedRef.current = true;
    void goCodingSession(channelId, generationId, { replace: true });
  }, [channelId, generationId, goCodingSession]);

  if (
    resolution.kind === "loading" ||
    resolution.kind === "missing" ||
    resolution.kind === "started" ||
    resolution.kind === "closed"
  ) {
    return (
      <CodingSessionFoundedState
        channelName={channelName}
        onBackToChannel={() => goChannel(channelId)}
        resolution={resolution}
        sessionRef={sessionRef}
        sessionTitle={sessionName}
      />
    );
  }

  const { founded } = resolution;
  const isFounder =
    currentUserPubkey !== null &&
    currentUserPubkey.toLowerCase() === founded.founderPubkey.toLowerCase();
  const starting = resolution.kind === "starting";
  const goalSentence = codingSessionFoundedGoalSentence(goal);
  return (
    <main
      className="flex h-full min-h-0 flex-1 flex-col bg-background"
      data-testid={`coding-session-founded-workspace-${resolution.kind}`}
    >
      <CodingSessionHeader
        channelName={channelName}
        founderDetails={founderName}
        generationLabel="not started"
        goalText={goal.kind === "available" ? goal.text : null}
        sessionTitle={sessionName}
        status={{ kind: "founded", label: "Not started" }}
      />
      <div className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto px-6 py-6">
        {isFounder ? null : (
          // The founder's own view has no title, goal or founder line here:
          // the header carries "not started" and the card carries the rest
          // (Andy, 2026-09-10). Everybody else is told what it is and whose.
          <div className="flex flex-col gap-2">
            <h1 className="text-base font-semibold">
              {sessionName?.trim() || "Untitled session"}
            </h1>
            {goal.kind === "available" ? (
              <p
                className="whitespace-pre-wrap text-sm"
                data-testid="coding-session-founded-goal"
              >
                {goal.text}
              </p>
            ) : (
              <p
                className="text-sm text-muted-foreground"
                data-testid="coding-session-founded-goal-missing"
              >
                {goalSentence}
              </p>
            )}
            <p
              className="text-sm text-muted-foreground"
              data-testid="coding-session-founded-founder"
            >
              {starting ? (
                <>
                  Founded by {founderName} — starting; the provider accepted a
                  create and its first report is still to come.
                </>
              ) : (
                <>Founded by {founderName} — only they can start it.</>
              )}
            </p>
          </div>
        )}
        {isFounder ? (
          setupCard(founded, starting)
        ) : (
          <CodingSessionFoundedSetupReadOnly
            founderName={founderName}
            starting={starting}
          />
        )}
      </div>
    </main>
  );
}

/**
 * The non-ready layout, as the workspace draws it for its own loading and
 * not-found states — copied rather than imported, because that file is at
 * its size ceiling and cannot export one more thing. `closed` is drawn here
 * too: a session closed before it started has nothing to set up.
 */
function CodingSessionFoundedState({
  channelName,
  onBackToChannel,
  resolution,
  sessionRef,
  sessionTitle,
}: {
  channelName: string | null;
  onBackToChannel: () => void;
  resolution: Extract<
    CodingSessionFoundedViewResolution,
    { kind: "loading" | "missing" | "started" | "closed" }
  >;
  sessionRef: string;
  sessionTitle: string | null;
}) {
  const loading =
    resolution.kind === "loading" || resolution.kind === "started";
  const closed = resolution.kind === "closed";
  return (
    <main
      className="flex h-full min-h-0 flex-1 flex-col bg-background"
      data-testid={`coding-session-founded-workspace-${resolution.kind}`}
    >
      <CodingSessionHeader
        channelName={channelName}
        generationLabel={closed ? "not started" : shortRef(sessionRef)}
        sessionTitle={closed ? sessionTitle : null}
        status={
          resolution.kind === "missing"
            ? { kind: "unknown", label: "Status unknown" }
            : closed
              ? { kind: "ended", label: "Ended" }
              : { kind: "founded", label: "Not started" }
        }
        statusLabelOverride={closed ? "Closed" : null}
      />
      <div className="flex min-h-0 flex-1 items-center justify-center px-6 py-10 text-center">
        <div className="max-w-md">
          {loading ? (
            <FuzzyLogo
              ariaLabel="Loading coding session"
              className="mx-auto text-muted-foreground"
              fuzz={false}
              loop
            />
          ) : (
            <CircleAlert className="mx-auto h-5 w-5 text-muted-foreground" />
          )}
          <h2 className="mt-4 text-base font-semibold">
            {resolution.kind === "started"
              ? "Opening the session"
              : resolution.kind === "loading"
                ? "Loading coding session"
                : closed
                  ? "Closed before it started"
                  : "Session not found"}
          </h2>
          <p className="mt-2 text-sm text-muted-foreground">
            {resolution.kind === "started"
              ? "This session has started; opening its generation."
              : resolution.kind === "loading"
                ? "Resolving the founded session from the relay catalog."
                : closed
                  ? "This session was closed before anything ran. It is filed under Settled on every device; Reopen it from its row in the sidebar to set it up and start it."
                  : resolution.description}
          </p>
          {closed ? (
            <Button
              className="mt-4"
              data-testid="coding-session-founded-back"
              onClick={onBackToChannel}
              type="button"
              variant="outline"
            >
              Back to the channel
            </Button>
          ) : null}
        </div>
      </div>
    </main>
  );
}

function shortRef(value: string): string {
  return value.length <= 28 ? value : `${value.slice(0, 28)}…`;
}
