import type { ChannelSection } from "@/features/sidebar/lib/useChannelSections";
import {
  sectionSortGroupKey,
  sortChannelsForSidebar,
  type ChannelSortGroupKey,
  type ChannelSortMode,
} from "@/features/sidebar/lib/channelSortPreference";
import type { Channel } from "@/shared/api/types";

/**
 * Buckets the global stream list into the user's custom sections plus the
 * unassigned remainder, applying each grouping's own sort preference.
 * Starred channels are excluded (they render in the Starred section).
 * Extracted verbatim from AppSidebar.
 */
export function buildSectionBuckets({
  streamChannels,
  channelSections,
  channelAssignments,
  starredChannelIds,
  sortModeFor,
}: {
  streamChannels: Channel[];
  channelSections: ChannelSection[];
  channelAssignments: Record<string, string>;
  starredChannelIds?: ReadonlySet<string>;
  sortModeFor: (group: ChannelSortGroupKey) => ChannelSortMode;
}): { bySection: Record<string, Channel[]>; unassigned: Channel[] } {
  const bySection: Record<string, Channel[]> = {};
  const unassigned: Channel[] = [];
  const sectionIds = new Set(channelSections.map((s) => s.id));

  for (const channel of streamChannels) {
    if (starredChannelIds?.has(channel.id)) continue;
    const sectionId = channelAssignments[channel.id];
    if (sectionId && sectionIds.has(sectionId)) {
      if (!bySection[sectionId]) {
        bySection[sectionId] = [];
      }
      bySection[sectionId].push(channel);
    } else {
      unassigned.push(channel);
    }
  }
  // Apply each grouping's own sort preference; section membership itself
  // is untouched.
  for (const sectionId of Object.keys(bySection)) {
    bySection[sectionId] = sortChannelsForSidebar(
      bySection[sectionId],
      sortModeFor(sectionSortGroupKey(sectionId)),
    );
  }
  return {
    bySection,
    unassigned: sortChannelsForSidebar(unassigned, sortModeFor("channels")),
  };
}
