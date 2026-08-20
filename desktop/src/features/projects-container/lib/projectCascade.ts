import { isSessionTransportChannel } from "@/shared/api/channelTypes";
import type { Channel } from "@/shared/api/types";
import type { Workflow } from "@/shared/api/workflowTypes";

import { normalizeProjectRef } from "./projectContainerModel";
import type { ProjectContainer } from "../hooks";

/**
 * The children a "delete everything" project delete would remove.
 *
 * Deliberately **not** the same set as the sidebar's `partitionChannels`:
 * that one splits streams from forums and drops transports on the floor,
 * because transports are machine surfaces nobody browses. A delete has to see
 * them — a transport channel admits project members through the project ACL
 * alone, so deleting the project makes the transport unreachable whether or
 * not the cascade box is ticked. Counting it here is the only way the dialog
 * can tell the truth about what disappears.
 */
export type ProjectCascadeTargets = {
  /** Every channel bound to the project, transports included. */
  channels: Channel[];
  /**
   * Workflows in those channels that the current identity **authored** — the
   * only ones it can actually delete.
   */
  workflows: Workflow[];
  /**
   * Workflows in those channels authored by somebody else.
   *
   * A workflow delete is a kind:5 `a`-tag tombstone, and a kind:5 only deletes
   * the *signer's own* addressable events. Issuing one for a teammate's
   * workflow signs `30620:<caller>:<their-id>` — a coordinate the caller owns
   * but which does not exist — and the relay accepts it, logs "no live row
   * matched coordinate", and returns `accepted: true`. The delete looks like a
   * success and the workflow is still alive. So these are separated out,
   * reported, and never issued. Mirrors the CLI's `foreign_workflows`.
   */
  foreignWorkflows: Workflow[];
};

/** Counts the delete dialog renders, one per child type. */
export type ProjectCascadeCounts = {
  channels: number;
  forums: number;
  transports: number;
  /** Workflows the cascade will actually delete. */
  workflows: number;
  /** Workflows it will leave alone because someone else authored them. */
  foreignWorkflows: number;
  /** Everything the cascade deletes, for the "nothing to do" check. */
  total: number;
};

/**
 * Whether `channel` belongs to `project`.
 *
 * Two independent bindings, matching `partitionByProject`: the project's own
 * curated `channelIds` forward refs, and the channel's `projectRef`
 * back-reference (the relay-maintained `channels.project_ref`).
 */
export function channelBelongsToProject(
  channel: Pick<Channel, "id" | "projectRef">,
  project: Pick<ProjectContainer, "address" | "channelIds">,
): boolean {
  if (project.channelIds.includes(channel.id)) return true;
  const ref = channel.projectRef;
  if (!ref) return false;
  return (normalizeProjectRef(ref) ?? ref) === project.address;
}

/**
 * Every channel a cascade delete of `project` would remove: streams, forums,
 * and transports. DMs are never project-bound and are excluded defensively.
 */
export function projectCascadeChannels(
  project: Pick<ProjectContainer, "address" | "channelIds">,
  channels: readonly Channel[],
): Channel[] {
  return channels.filter(
    (channel) =>
      channel.channelType !== "dm" && channelBelongsToProject(channel, project),
  );
}

/**
 * Split the workflows defined in `channelIds` into the ones `selfPubkey`
 * authored (deletable) and everybody else's (reported, never deleted).
 *
 * `selfPubkey` is compared case-insensitively; pass `null` while the identity
 * is still loading, which classifies **every** workflow as foreign — the safe
 * direction, since a foreign classification only ever withholds a delete.
 */
export function projectCascadeWorkflows(
  channelIds: readonly string[],
  workflows: readonly Workflow[],
  selfPubkey: string | null,
): { mine: Workflow[]; foreign: Workflow[] } {
  const ids = new Set(channelIds);
  const self = selfPubkey?.toLowerCase() ?? null;
  const mine: Workflow[] = [];
  const foreign: Workflow[] = [];
  for (const workflow of workflows) {
    if (workflow.channelId === null || !ids.has(workflow.channelId)) continue;
    if (self !== null && workflow.ownerPubkey.toLowerCase() === self) {
      mine.push(workflow);
    } else {
      foreign.push(workflow);
    }
  }
  return { mine, foreign };
}

/** Per-type counts for the delete dialog. */
export function projectCascadeCounts(
  targets: ProjectCascadeTargets,
): ProjectCascadeCounts {
  let channels = 0;
  let forums = 0;
  let transports = 0;
  for (const channel of targets.channels) {
    if (isSessionTransportChannel(channel)) transports += 1;
    else if (channel.channelType === "forum") forums += 1;
    else channels += 1;
  }
  return {
    channels,
    forums,
    transports,
    workflows: targets.workflows.length,
    foreignWorkflows: targets.foreignWorkflows.length,
    // Only what is actually deleted. Foreign workflows are deliberately out:
    // counting them here would arm "delete everything" for a project whose
    // only children the caller cannot touch.
    total: targets.channels.length + targets.workflows.length,
  };
}

/**
 * The one-line inventory the dialog shows under the cascade checkbox, e.g.
 * `2 channels, 1 forum, 1 session transport, 3 workflows`. Returns an empty
 * string when there is nothing to delete, so the caller can say so plainly
 * instead of rendering "0 channels".
 */
export function describeProjectCascade(counts: ProjectCascadeCounts): string {
  const parts: string[] = [];
  const push = (n: number, one: string, many: string) => {
    if (n > 0) parts.push(`${n} ${n === 1 ? one : many}`);
  };
  push(counts.channels, "channel", "channels");
  push(counts.forums, "forum", "forums");
  push(counts.transports, "session transport", "session transports");
  push(counts.workflows, "workflow", "workflows");
  return parts.join(", ");
}

/**
 * The things a cascade will **not** do, as sentences for the dialog.
 *
 * Anything the cascade skips has to be named where the user is deciding, not
 * discovered afterwards: a dialog that counts three workflows, deletes one,
 * and reports success is the same class of bug as a crash.
 *
 * `workflowsUnknown` is set when this build cannot enumerate workflows at all
 * (the `workflows` feature is off, or the lookup failed). The channels are
 * still deleted, and any workflow defined in them is orphaned rather than
 * removed — so the dialog says so instead of quietly omitting workflows from
 * the count.
 */
export function projectCascadeExclusionNotes(
  counts: ProjectCascadeCounts,
  workflowsUnknown: boolean,
): string[] {
  const notes: string[] = [];
  if (workflowsUnknown) {
    notes.push(
      "Workflows in these channels can't be listed in this build, so they are not deleted. Any workflow defined in a deleted channel is left behind.",
    );
  } else if (counts.foreignWorkflows > 0) {
    notes.push(
      counts.foreignWorkflows === 1
        ? "1 workflow in these channels was created by someone else and will not be deleted — only its author can delete it."
        : `${counts.foreignWorkflows} workflows in these channels were created by someone else and will not be deleted — only their authors can delete them.`,
    );
  }
  return notes;
}
