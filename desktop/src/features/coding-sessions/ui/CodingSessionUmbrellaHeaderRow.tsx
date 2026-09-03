import type * as React from "react";

import type { CodingSessionLens } from "@/features/coding-sessions/lib/codingSessionLensPreference";
import type { CodingSessionMissionDensity } from "@/features/coding-sessions/lib/codingSessionMissionDensity";
import type {
  CodingSessionExecution,
  CodingSessionUmbrellaRecord,
} from "@/features/coding-sessions/lib/codingSessionTypes";
import type { CodingSessionGoal } from "@/features/coding-sessions/lib/codingSessionGoal";
import type { CodingSessionSurface } from "@/features/coding-sessions/lib/codingSessionRoute";
import type { CodingSessionActorNameResolver } from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import type { SeatBeeStamp } from "@/features/coding-sessions/lib/codingSessionSeatBee";
import type { CodingSessionReachabilityResolver } from "@/features/coding-sessions/hooks/useCodingSessionProviderReachability";
import type { useCodingSessionTeamWake } from "@/features/coding-sessions/hooks/useCodingSessionTeamWake";
import {
  CodingSessionDispositionStrip,
  CodingSessionHeader,
} from "./CodingSessionHeader";
import { CodingSessionFounderLine } from "./CodingSessionFounderLine";
import {
  CodingSessionAgentFocus,
  type CodingSessionAgentFocusItem,
} from "./CodingSessionAgentFocus";
import { CodingSessionLensControl } from "./CodingSessionLensControl";
import { CodingSessionParticipantBar } from "./CodingSessionParticipantBar";
import { CodingSessionMissionDensityControl } from "./CodingSessionMissionDensityControl";
import type { useCodingSessionTaskDock } from "./useCodingSessionTaskDock";
import type { useCodingSessionSurfaceHostState } from "./CodingSessionSurfaceHost";
import type { CodingSessionRouteRailState } from "./useCodingSessionRoute";
import type { CodingSessionSurfaceDescriptor } from "./CodingSessionSurfaceHost";
import type { CodingSessionStopAllModel } from "@/features/coding-sessions/lib/codingSessionStopAllModel";
import {
  codingSessionMissionUmbrellaWord,
  umbrellaAgentStatusSummary,
  umbrellaWorkspaceStatus,
} from "./CodingSessionUmbrellaWorkspaceModel";
import { codingSessionUmbrellaGenerationLabel } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";

type HeaderProps = React.ComponentProps<typeof CodingSessionHeader>;

/**
 * The authority summary block: the umbrella's header plus its second row —
 * the Mission participant bar or Conversation's disposition strip.
 *
 * Pulled out of `CodingSessionUmbrellaWorkspace.tsx` verbatim (REVIEW-L4 F1's
 * second consequence: the file crossed the 1,000-line ceiling once L2's merge
 * and the fix round's own additions landed). Nothing moved but the code —
 * every prop here is a value the workspace already computed.
 */
export function CodingSessionUmbrellaHeaderRow({
  agentFocusItems,
  authoritativeTitle,
  canRename,
  channelName,
  composerTaskDock,
  contextLoads,
  focusedExecution,
  focusedExecutionKey,
  goal,
  handleFocusExecution,
  handleLensChange,
  handleMissionDensityChange,
  handlePopout,
  handleStopAll,
  handleToggleRouteRail,
  headerCompact,
  isMultiExecution,
  isNarrow,
  lens,
  mission,
  missionDensity,
  onAddProvider,
  onClose,
  onCloseSession,
  onOpenPeople,
  onReopenSession,
  peopleCount,
  resolveReachability,
  routedSeats,
  routeRail,
  seatBeeStamps,
  sessionClosed,
  setRenameOpen,
  streamParticipants,
  stopAll,
  surface,
  surfaceHost,
  surfaceHostId,
  surfaces,
  teamWake,
  umbrella,
  workspaceActorName,
}: {
  agentFocusItems: CodingSessionAgentFocusItem[];
  authoritativeTitle: string;
  canRename: boolean;
  channelName: string | null;
  composerTaskDock: ReturnType<typeof useCodingSessionTaskDock>;
  contextLoads: HeaderProps["contextLoads"];
  focusedExecution: CodingSessionExecution;
  focusedExecutionKey: string | null;
  goal: CodingSessionGoal | null;
  handleFocusExecution: (key: string | null) => void;
  handleLensChange: (lens: CodingSessionLens) => void;
  handleMissionDensityChange: (density: CodingSessionMissionDensity) => void;
  handlePopout: () => void;
  handleStopAll: () => void;
  handleToggleRouteRail: () => void;
  headerCompact: boolean;
  isMultiExecution: boolean;
  isNarrow: boolean;
  lens: CodingSessionLens;
  mission: boolean;
  missionDensity: CodingSessionMissionDensity;
  onAddProvider?: () => void;
  onClose?: () => void;
  onCloseSession?: () => void;
  onOpenPeople?: () => void;
  onReopenSession?: () => void;
  peopleCount: number;
  resolveReachability: CodingSessionReachabilityResolver;
  routedSeats: HeaderProps["routedSeats"];
  routeRail: CodingSessionRouteRailState;
  /** Which `bee` each seat is running, keyed by `executionKey` (L12/L17). */
  seatBeeStamps: ReadonlyMap<string, SeatBeeStamp | null>;
  sessionClosed: boolean;
  setRenameOpen: (open: boolean) => void;
  streamParticipants: React.ComponentProps<
    typeof CodingSessionParticipantBar
  >["items"];
  stopAll: CodingSessionStopAllModel;
  surface: CodingSessionSurface;
  surfaceHost: ReturnType<typeof useCodingSessionSurfaceHostState>;
  surfaceHostId: string;
  surfaces: CodingSessionSurfaceDescriptor[];
  teamWake: ReturnType<typeof useCodingSessionTeamWake>;
  umbrella: CodingSessionUmbrellaRecord;
  workspaceActorName: CodingSessionActorNameResolver;
}) {
  return (
    <div className="shrink-0" data-testid="coding-session-authority-summary">
      <CodingSessionHeader
        agentControls={
          isMultiExecution && !isNarrow && !mission ? (
            <CodingSessionAgentFocus
              agentSurfaceOpen={surfaceHost.activeTab === "agents"}
              focusedExecutionKey={focusedExecutionKey}
              items={agentFocusItems}
              onFocus={handleFocusExecution}
              onOpenAgents={() => {
                composerTaskDock.close();
                surfaceHost.toggle("agents");
              }}
              surfaceHostId={surfaceHostId}
            />
          ) : undefined
        }
        channelName={channelName}
        compact={isNarrow || headerCompact}
        // In Mission the rail owns both of these — Team for the seats,
        // the Context tab for the load table — so the provenance popover
        // keeps only what is genuinely provenance (founder, verified
        // source) instead of showing a third copy of the roster.
        contextLoads={mission ? undefined : contextLoads}
        routedSeats={mission ? undefined : routedSeats}
        // Mission's header is a two-row container; the participant bar below
        // carries the single bottom rule.
        flush={mission}
        founderDetails={
          umbrella.founderPubkey ? (
            <CodingSessionFounderLine
              founderPubkey={umbrella.founderPubkey}
              genesisRef={umbrella.genesisRef}
              variant="label"
            />
          ) : undefined
        }
        generationLabel={codingSessionUmbrellaGenerationLabel(umbrella)}
        goalText={goal?.content ?? null}
        onAddProvider={onAddProvider}
        onClose={onClose}
        onCloseSession={onCloseSession}
        onOpenPeople={onOpenPeople}
        onPopout={surface === "main" ? handlePopout : undefined}
        onRename={canRename ? () => setRenameOpen(true) : undefined}
        onReopenSession={onReopenSession}
        onStopAll={stopAll.kind === "available" ? handleStopAll : undefined}
        stopAllCount={stopAll.kind === "available" ? stopAll.seatCount : 0}
        stopAllLabel={stopAll.kind === "available" ? stopAll.buttonLabel : null}
        stopAllSentence={stopAll.kind === "available" ? stopAll.sentence : null}
        peopleCount={peopleCount}
        onToggleTaskRail={
          !isMultiExecution && composerTaskDock.activeModel
            ? () => {
                surfaceHost.close();
                composerTaskDock.toggle();
              }
            : undefined
        }
        onToggleSurface={(id) => {
          composerTaskDock.close();
          surfaceHost.toggle(id);
        }}
        providerAuthorityPubkey={focusedExecution.signerPubkey}
        sessionTitle={authoritativeTitle}
        sessionClosed={sessionClosed}
        status={umbrellaWorkspaceStatus(umbrella)}
        // SURFACES A3/B2: Mission's header shows the demoted lifecycle word
        // and nothing else. The `2 AGENTS · 2 WORKING` aggregate it used to
        // carry restated — less precisely, and one row above — what the
        // roster chips and the live strip already say per seat, and it read
        // `IDLE` over a session with a seat mid-turn on the 2026-09-01 run.
        missionActions={mission}
        onToggleRouteRail={mission ? handleToggleRouteRail : undefined}
        routeRailDisplacesInspector={!routeRail.roomForRail}
        routeRailExpanded={routeRail.fits}
        statusLabelOverride={
          mission
            ? codingSessionMissionUmbrellaWord(
                umbrellaWorkspaceStatus(umbrella),
              )
            : umbrellaAgentStatusSummary(agentFocusItems)
        }
        surfaceHostId={surfaceHostId}
        surfaceTabs={surfaces
          .filter(
            (surfaceEntry) =>
              surfaceEntry.id !== "agents" &&
              (!mission ||
                (surfaceHost.activeTab === null &&
                  surfaceEntry.id === "mission-inspector")),
          )
          .map((surfaceEntry) => ({
            id: surfaceEntry.id,
            label: surfaceEntry.label,
            icon:
              surfaceEntry.id === "agents"
                ? "agents"
                : surfaceEntry.id === "mission-inspector" ||
                    surfaceEntry.id === "mission-context" ||
                    surfaceEntry.id === "mission-audit"
                  ? "inspector"
                  : "changes",
            count: surfaceEntry.count ?? 0,
            active: surfaceHost.activeTab === surfaceEntry.id,
          }))}
        taskCount={
          isMultiExecution
            ? 0
            : (composerTaskDock.activeModel?.tasks.length ?? 0)
        }
        taskRailOpen={composerTaskDock.open}
        viewControl={
          isMultiExecution ? (
            <CodingSessionLensControl
              compact
              lens={lens}
              onChange={handleLensChange}
            />
          ) : undefined
        }
      />
      {mission ? (
        <CodingSessionParticipantBar
          // Row 2 of the header container: the bar stopped drawing its own
          // band so this is the container's one bottom rule.
          className="border-b border-border/60 bg-background/80"
          deliveries={teamWake.deliveries}
          focusedExecutionKey={focusedExecutionKey}
          items={streamParticipants}
          leading={
            <CodingSessionMissionDensityControl
              density={missionDensity}
              onChange={handleMissionDensityChange}
            />
          }
          onFocus={handleFocusExecution}
          // A chip's delivery badge is matched to its seat through the
          // seat's actorPubkey, which only this projection carries; without
          // it the chips would badge nothing at all.
          seatAuthorities={teamWake.seatAuthorities}
          seatBeeStamps={seatBeeStamps}
        />
      ) : isMultiExecution ? (
        <CodingSessionDispositionStrip
          actorNames={workspaceActorName}
          resolveReachability={resolveReachability}
          umbrella={umbrella}
        />
      ) : null}
    </div>
  );
}
