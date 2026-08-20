/**
 * The project's channel set — the only place Pulse reads session facts from.
 *
 * `channels.project_ref` is the relation the relay joins on, and this mirrors
 * it directly: every channel whose own `projectRef` names the project counts,
 * **whatever its `channelType`**.
 *
 * That last clause is load-bearing. `partitionChannels` keeps only `stream`
 * and `forum` channels, and coding-session facts (44223/44227/44229/44230)
 * live in the project's `transport` channel — dropped by that filter before
 * the back-reference is ever consulted. The 30621 head's owner-curated
 * `channel` forward ref only rescues the case where the session channel's
 * creator is the project owner, and even then it is explicitly best-effort.
 * The CLI resolves the same set from the kind:39000 `project` tag, which the
 * relay emits for transports too, so without the back-reference pass Desktop
 * would render "the project is quiet" over sessions `buzz pulse sessions`
 * lists.
 */
import type { Channel } from "@/shared/api/channelTypes";
import { normalizeProjectRef } from "@/features/projects-container/lib/projectContainerModel";

/** Minimal shape this module needs from a project container. */
export type PulseChannelSetProject = {
  id: string;
  address: string;
  channelIds: readonly string[];
};

/** Minimal shape this module needs from a channel. */
export type PulseChannelSetChannel = Pick<Channel, "id"> & {
  projectRef?: string | null;
};

/**
 * Union of three sources, deduplicated and sorted:
 *
 * 1. the project head's curated `channel` forward refs,
 * 2. the stream/forum buckets the sidebar already computed, and
 * 3. every channel carrying a `projectRef` back-reference to this project,
 *    regardless of type.
 *
 * `bucketed` is passed in rather than recomputed so this stays a pure
 * function over data the caller already has.
 */
export function projectPulseChannelIds(
  project: PulseChannelSetProject,
  channels: readonly PulseChannelSetChannel[],
  bucketed: readonly string[] = [],
): string[] {
  const ids = new Set<string>([
    ...project.channelIds,
    ...bucketed,
    ...channels
      .filter((channel) => {
        const ref = channel.projectRef;
        if (!ref) return false;
        return (normalizeProjectRef(ref) ?? ref) === project.address;
      })
      .map((channel) => channel.id),
  ]);
  return [...ids].sort();
}
