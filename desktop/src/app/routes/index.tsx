import * as React from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";

import { useAgentProgress } from "@/app/agentProgressComposition";
import { useAppNavigation } from "@/app/navigation/useAppNavigation";
import { useChannelsQuery } from "@/features/channels/hooks";
import {
  type DashboardTab,
  parseDashboardTab,
  resolveDashboardTab,
} from "@/features/dashboard/lib/dashboardTabs";
import { DashboardScreen } from "@/features/dashboard/ui/DashboardScreen";
import {
  consumePendingWelcomeChannel,
  WELCOME_CHANNEL_READY_EVENT,
} from "@/features/onboarding/welcome";
import {
  parseProfilePanelTab,
  parseProfilePanelView,
  type ProfilePanelTab,
  type ProfilePanelView,
} from "@/features/profile/ui/UserProfilePanelUtils";
import { useIdentityQuery } from "@/shared/api/hooks";
import { useFeatureEnabled, usePreviewFeatureWarning } from "@/shared/features";
import { ViewLoadingFallback } from "@/shared/ui/ViewLoadingFallback";

const HomeScreen = React.lazy(async () => {
  const module = await import("@/features/home/ui/HomeScreen");
  return { default: module.HomeScreen };
});

const PulseScreen = React.lazy(async () => {
  const module = await import("@/features/pulse/ui/PulseScreen");
  return { default: module.PulseScreen };
});

const AgentProgressScreen = React.lazy(async () => {
  const module = await import(
    "@/features/agent-progress/ui/AgentProgressScreen"
  );
  return { default: module.AgentProgressScreen };
});

const AgentsScreen = React.lazy(async () => {
  const module = await import("@/features/agents/ui/AgentsScreen");
  return { default: module.AgentsScreen };
});

/**
 * The union of every search key the tab bodies read through
 * `useHistorySearchState`: `item` for the inbox, the profile-panel keys for
 * Pulse and Agents. Anything not returned here is stripped on the next
 * search patch, so each body's keys must survive the route boundary.
 */
type DashboardRouteSearch = {
  tab?: Exclude<DashboardTab, "overview">;
  item?: string;
  profile?: string;
  profilePersona?: string;
  profileTab?: ProfilePanelTab;
  profileView?: ProfilePanelView;
};

function nonEmptyString(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function validateDashboardSearch(
  search: Record<string, unknown>,
): DashboardRouteSearch {
  const tab = parseDashboardTab(search);
  return {
    tab: tab === "overview" ? undefined : tab,
    item: nonEmptyString(search.item),
    profile: nonEmptyString(search.profile),
    profilePersona: nonEmptyString(search.profilePersona),
    profileTab: parseProfilePanelTab(search.profileTab) ?? undefined,
    profileView: parseProfilePanelView(search.profileView) ?? undefined,
  };
}

export const Route = createFileRoute("/")({
  validateSearch: validateDashboardSearch,
  component: DashboardRouteComponent,
});

function DashboardRouteComponent() {
  const { tab } = Route.useSearch();
  const requested: DashboardTab = tab ?? "overview";
  const showPulse = useFeatureEnabled("pulse");
  const showAgentProgress = useFeatureEnabled("agent-progress");
  const active = resolveDashboardTab(requested, {
    pulse: showPulse,
    agentProgress: showAgentProgress,
  });
  // The warning names the preview the URL asked for, whether or not the gate
  // let it render — same as the standalone routes did. Tab ids that are not
  // manifest feature ids (overview, inbox, agents) make the hook a no-op.
  usePreviewFeatureWarning(requested);
  useWelcomeChannelRedirect();

  return (
    <DashboardScreen
      active={active}
      agentProgress={
        <React.Suspense fallback={<ViewLoadingFallback kind="agents" />}>
          <AgentProgressTab />
        </React.Suspense>
      }
      agents={
        <React.Suspense fallback={<ViewLoadingFallback kind="agents" />}>
          <AgentsScreen />
        </React.Suspense>
      }
      inbox={
        <React.Suspense fallback={<ViewLoadingFallback kind="projects" />}>
          <InboxTab />
        </React.Suspense>
      }
      pulse={
        <React.Suspense fallback={<ViewLoadingFallback kind="pulse" />}>
          <PulseScreen />
        </React.Suspense>
      }
      showAgentProgress={showAgentProgress}
      showPulse={showPulse}
    />
  );
}

function useAvailableChannelIds() {
  const channelsQuery = useChannelsQuery();
  const channels = channelsQuery.data ?? [];
  return React.useMemo(
    () => new Set(channels.map((channel) => channel.id)),
    [channels],
  );
}

/**
 * Onboarding parks the welcome channel until the channel list can prove it
 * exists; this route is where that promise is kept, on whichever tab the
 * user landed.
 */
function useWelcomeChannelRedirect() {
  const { goChannel } = useAppNavigation();
  const availableChannelIds = useAvailableChannelIds();
  const availableChannelIdsRef = React.useRef(availableChannelIds);
  const openPendingWelcomeChannel = React.useCallback(
    (ids: ReadonlySet<string>) => {
      const welcomeChannelId = consumePendingWelcomeChannel(ids);
      if (!welcomeChannelId) {
        return;
      }

      void goChannel(welcomeChannelId, { replace: true });
    },
    [goChannel],
  );

  React.useEffect(() => {
    availableChannelIdsRef.current = availableChannelIds;
  }, [availableChannelIds]);

  React.useEffect(() => {
    function handleWelcomeChannelReady() {
      openPendingWelcomeChannel(availableChannelIdsRef.current);
    }

    window.addEventListener(
      WELCOME_CHANNEL_READY_EVENT,
      handleWelcomeChannelReady,
    );
    return () => {
      window.removeEventListener(
        WELCOME_CHANNEL_READY_EVENT,
        handleWelcomeChannelReady,
      );
    };
  }, [openPendingWelcomeChannel]);

  React.useEffect(() => {
    openPendingWelcomeChannel(availableChannelIds);
  }, [availableChannelIds, openPendingWelcomeChannel]);
}

function InboxTab() {
  const { goChannel } = useAppNavigation();
  const identityQuery = useIdentityQuery();
  const availableChannelIds = useAvailableChannelIds();
  return (
    <HomeScreen
      availableChannelIds={availableChannelIds}
      currentPubkey={identityQuery.data?.pubkey}
      onOpenContext={(channelId, messageId, threadRootId) => {
        void goChannel(channelId, { messageId, threadRootId });
      }}
    />
  );
}

function AgentProgressTab() {
  const navigate = useNavigate();
  const state = useAgentProgress();
  return (
    <AgentProgressScreen
      state={state}
      onOpenLane={(lane) => {
        if (!lane.openTarget) return;
        void navigate({
          to: "/coding-sessions/$channelId/$generationId",
          params: {
            channelId: lane.openTarget.channelId,
            generationId: lane.openTarget.generationId,
          },
          search: { surface: "main" },
        });
      }}
    />
  );
}
