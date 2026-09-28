import * as React from "react";
import type { ForcedUnreadSource } from "@/features/channels/forcedUnreadStore";
import type { ContextParentResolver } from "@/features/channels/readState/readStateManager";
import type { ThreadActivityItem } from "@/features/channels/useUnreadChannels";
import type { FeedItemState } from "@/features/home/useFeedItemState";
import type { FeedItem } from "@/shared/api/types";
import type { SettingsSection } from "@/features/settings/ui/SettingsPanels";

const EMPTY_SET = new Set<string>();
const EMPTY_COUNT_MAP = new Map<string, number>();

type AppShellContextValue = {
  markAllChannelsRead: () => void;
  markChannelRead: (
    channelId: string,
    readAt: string | null | undefined,
    options?: {
      preserveForcedUnread?: boolean;
      topLevelOnly?: boolean;
    },
  ) => void;
  markChannelUnread: (channelId: string, source?: ForcedUnreadSource) => void;
  clearChannelUnreadSource: (
    channelId: string,
    source: ForcedUnreadSource,
  ) => void;
  openBrowseChannels: () => void;
  openCreateChannel: () => void;
  openChannelManagement: (channelId?: string) => void;
  // NIP-RS read marker for a channel as a unix-seconds timestamp, or null
  // when unknown. Backed by the single AppShell-mounted ReadStateManager so
  // every surface (sidebar, home, badges) projects from the same source.
  getChannelReadAt: (channelId: string) => number | null;
  // Thread read frontier as unix-seconds timestamp, or null when never read.
  // Uses `thread:<rootId>` context keys in the same ReadStateManager.
  getThreadReadAt: (rootId: string, channelId?: string | null) => number | null;
  // Advance the thread read frontier to the given unix-seconds timestamp.
  markThreadRead: (rootId: string, timestamp: number) => void;
  // Per-message read frontier as unix-seconds timestamp, or null when never
  // read. Uses `msg:<id>` context keys folded through the active channel by the
  // parent resolver (LP4 v3 per-message badge model).
  getMessageReadAt: (messageId: string) => number | null;
  // Read frontier for a channel-activity item, scoped to that item's own
  // message, its thread's aggregate marker and its channel, rather than the
  // currently mounted channel resolver. `tags` is required because the thread
  // root is read from the row's own `root` e-tag (ledger 279(g)).
  getChannelActivityItemReadAt: (
    item: Pick<FeedItem, "channelId" | "id" | "tags">,
  ) => number | null;
  // Advance a single message's read marker to the given unix-seconds timestamp.
  markMessageRead: (messageId: string, timestamp: number) => void;
  // Bump-counter that invalidates whenever the read marker changes. Include
  // in memo deps that consume getChannelReadAt.
  readStateVersion: number;
  // Inject the thread→channel parent resolver derived from the event graph
  // (NIP-RS hierarchical frontier). Set by the active channel surface.
  setContextParentResolver: (resolver: ContextParentResolver | null) => void;
  followThread: (rootId: string) => void;
  unfollowThread: (rootId: string) => void;
  isFollowingThread: (rootId: string) => boolean;
  isNotifiedForThread: (rootId: string) => boolean;
  recordThreadInteraction: (rootId: string) => void;
  isThreadMuted: (rootId: string) => boolean;
  // Raw, UNRECONCILED thread-activity buffer straight out of localStorage +
  // live traffic. It can still contain rows whose events no longer exist on the
  // relay. Do not render from this — render `threadActivityFeedItems`, which is
  // the same list after the relay-existence reconcile in
  // app/useChannelActivityProjection.ts has dropped confirmed-absent rows.
  threadActivityItems: ThreadActivityItem[];
  threadActivityFeedItems: FeedItem[];
  // Home-feed items explicitly reopened from Inbox. Kept separate from live
  // thread activity so older rows can be projected into channel hover cards
  // without duplicating the Home feed itself.
  locallyUnreadFeedItems: FeedItem[];
  // Thread rows that remain unread until their own message/thread marker is
  // advanced. Unlike the broad channel unread set, this includes the active
  // channel so simply landing in it does not hide the wayfinding signal.
  unreadThreadFeedItems: FeedItem[];
  unreadThreadChannelIds: ReadonlySet<string>;
  // Ordinary unread channel-level activity. Sidebar rows use this for text
  // emphasis only; thread activity owns the dot.
  topLevelUnreadChannelIds: ReadonlySet<string>;
  // Per-channel count of every unread message that concerns this user —
  // top-level messages plus replies in threads they are part of, plus
  // mentions. This is what the sidebar row's unread count badge shows, so it
  // deliberately counts more than `unreadChannelCounts` does for a regular
  // channel. Channels with no unread messages are absent, as are channels
  // that are unread only because the user chose "Mark unread".
  unreadChannelTotals: ReadonlyMap<string, number>;
  // Lets isolated component tests retain the legacy hasUnread fallback while
  // the mounted shell uses the split projections above.
  hasSidebarUnreadProjections: boolean;
  feedItemState: FeedItemState;
  // Open the Settings panel at the given section. Available on all surfaces
  // that render under AppShell (channel, home, projects, pulse, agents).
  // Used by config-nudge cards to deep-link to Settings → Agents.
  onOpenSettings: ((section: SettingsSection) => void) | null;
  // The number the Dashboard sidebar row wears: unread inbox items plus due
  // reminders, computed once in AppShell. The Dashboard overview reads it
  // from here rather than re-running the notification fold, which owns
  // seen-set side effects and must mount exactly once.
  inboxBadgeCount: number;
};

const AppShellContext = React.createContext<AppShellContextValue>({
  markAllChannelsRead: () => {},
  markChannelRead: () => {},
  markChannelUnread: () => {},
  clearChannelUnreadSource: () => {},
  openBrowseChannels: () => {},
  openCreateChannel: () => {},
  openChannelManagement: () => {},
  getChannelReadAt: () => null,
  getThreadReadAt: () => null,
  markThreadRead: () => {},
  getMessageReadAt: () => null,
  getChannelActivityItemReadAt: () => null,
  markMessageRead: () => {},
  readStateVersion: 0,
  setContextParentResolver: () => {},
  followThread: () => {},
  unfollowThread: () => {},
  isFollowingThread: () => false,
  isNotifiedForThread: () => false,
  recordThreadInteraction: () => {},
  isThreadMuted: () => false,
  threadActivityItems: [],
  threadActivityFeedItems: [],
  locallyUnreadFeedItems: [],
  unreadThreadFeedItems: [],
  unreadThreadChannelIds: EMPTY_SET,
  topLevelUnreadChannelIds: EMPTY_SET,
  unreadChannelTotals: EMPTY_COUNT_MAP,
  hasSidebarUnreadProjections: false,
  feedItemState: {
    doneSet: EMPTY_SET,
    markDone: () => {},
    markUnread: () => {},
    undoDone: () => {},
    undoUnread: () => {},
    unreadSet: EMPTY_SET,
  },
  onOpenSettings: null,
  inboxBadgeCount: 0,
});

export function AppShellProvider({
  children,
  value,
}: {
  children: React.ReactNode;
  value: AppShellContextValue;
}) {
  return (
    <AppShellContext.Provider value={value}>
      {children}
    </AppShellContext.Provider>
  );
}

export function useAppShell() {
  return React.useContext(AppShellContext);
}
