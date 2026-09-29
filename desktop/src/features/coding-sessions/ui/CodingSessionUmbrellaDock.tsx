import type * as React from "react";

import { cn } from "@/shared/lib/cn";

import {
  CodingSessionActiveWorkDock,
  type CodingSessionActiveWorkAgent,
} from "./CodingSessionActiveWorkDock";
import {
  CodingSessionColumn,
  CODING_SESSION_COMPOSER_DOCK_CLASS,
} from "./CodingSessionColumn";
import { CodingSessionLiveActivityBar } from "./CodingSessionLiveActivityBar";
import { CodingSessionTaskRail } from "./CodingSessionTaskRail";
import {
  CodingSessionUmbrellaComposer,
  type CodingSessionUmbrellaComposerPrefill,
} from "./CodingSessionUmbrellaComposer";
import type { CodingSessionChannelAccess } from "@/features/coding-sessions/lib/codingSessionChannelAccess";
import type { CodingSessionTaskModel } from "@/features/coding-sessions/lib/codingSessionTaskModel";
import type { CodingSessionStreamPresence } from "@/features/coding-sessions/lib/codingSessionStreamPresence";
import type { CodingSessionReachabilityResolver } from "@/features/coding-sessions/hooks/useCodingSessionProviderReachability";
import type { CodingSessionActorNameResolver } from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import type { CodingSessionUmbrellaRecord } from "@/features/coding-sessions/lib/codingSessionTypes";

/**
 * The workspace's bottom dock: the strip above the composer, and the composer.
 *
 * Split out of `CodingSessionUmbrellaWorkspace.tsx`, which sits at the
 * repository's 1,000-line ceiling — the rule is to split the file, never to
 * raise the limit. Nothing about the dock's own markup changed in the move.
 *
 * `dockRef` is the one addition, and it is why the split is here rather than
 * somewhere arbitrary: B4's fix measures *this element's* height and reserves
 * exactly that much under the stream's last row, so the unreachable notice and
 * the task dock push the reserve instead of landing on a turn block.
 */
export function CodingSessionUmbrellaDock({
  acceptedOperators,
  activeWorkAgents,
  actorNames,
  channelId,
  currentUserPubkey,
  dockRef,
  focusedExecutionKey,
  gutter,
  channelAccess,
  isMultiExecution,
  isNarrow,
  mission,
  narrativeExpanded,
  onAddProvider,
  onFocusExecution,
  onSelectedParticipantChange,
  prefill,
  resolveReachability,
  streamPresence,
  taskDock,
  umbrella,
}: {
  acceptedOperators: React.ComponentProps<
    typeof CodingSessionUmbrellaComposer
  >["acceptedOperators"];
  activeWorkAgents: readonly CodingSessionActiveWorkAgent[];
  actorNames: CodingSessionActorNameResolver;
  channelId: string;
  currentUserPubkey: string | null;
  dockRef: React.RefObject<HTMLDivElement | null>;
  focusedExecutionKey: string | null;
  gutter: string;
  channelAccess: CodingSessionChannelAccess;
  isMultiExecution: boolean;
  isNarrow: boolean;
  mission: boolean;
  narrativeExpanded: boolean;
  onAddProvider?: () => void;
  onFocusExecution: (executionKey: string | null) => void;
  onSelectedParticipantChange: (key: string | null) => void;
  prefill: CodingSessionUmbrellaComposerPrefill | null;
  resolveReachability: CodingSessionReachabilityResolver;
  streamPresence: CodingSessionStreamPresence;
  taskDock: {
    activeModel: CodingSessionTaskModel | null;
    close: () => void;
    open: boolean;
  };
  umbrella: CodingSessionUmbrellaRecord;
}) {
  return (
    <div
      className={cn(CODING_SESSION_COMPOSER_DOCK_CLASS, gutter)}
      // Both lenses: the stream's reserve is measured off this element in
      // each of them now (Conversation's literal `pb-48` was the bug), so the
      // tests that prove the reserve equals the dock need to find it in both.
      data-testid="coding-session-composer-dock"
      ref={dockRef}
    >
      <CodingSessionColumn
        className="pointer-events-auto"
        expanded={narrativeExpanded}
        mission={mission}
      >
        {mission ? (
          <div className="-mb-6">
            <CodingSessionLiveActivityBar
              items={streamPresence.liveActivity}
              onFocus={onFocusExecution}
            />
          </div>
        ) : isMultiExecution ? (
          <div className="-mb-6">
            <CodingSessionActiveWorkDock
              agents={activeWorkAgents}
              focusedExecutionKey={focusedExecutionKey}
              onFocusAgent={onFocusExecution}
            />
          </div>
        ) : taskDock.open && !isNarrow ? (
          <div className="-mb-6">
            <CodingSessionTaskRail
              model={taskDock.activeModel}
              onClose={taskDock.close}
              variant="dock"
            />
          </div>
        ) : null}
        <CodingSessionUmbrellaComposer
          actorNames={actorNames}
          acceptedOperators={acceptedOperators}
          channelId={channelId}
          currentUserPubkey={currentUserPubkey}
          channelAccess={channelAccess}
          onAddProvider={onAddProvider}
          onSelectedParticipantChange={onSelectedParticipantChange}
          prefill={prefill}
          resolveReachability={resolveReachability}
          umbrella={umbrella}
        />
      </CodingSessionColumn>
    </div>
  );
}
