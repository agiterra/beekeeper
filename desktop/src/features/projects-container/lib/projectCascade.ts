import type { Repository } from "@/features/projects/projectModels";
import { isSessionTransportChannel } from "@/shared/api/channelTypes";
import type { Channel, RelayEvent } from "@/shared/api/types";
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
  /**
   * Repositories this identity may actually delete — armed by a separate
   * checkbox, never by the main cascade tick.
   */
  repos: CascadeRepo[];
  /**
   * Repositories bound to the project that this identity cannot delete.
   *
   * Reported, never issued a delete. See {@link projectCascadeRepos} for why
   * the rule is not simply "events I signed".
   */
  foreignRepos: CascadeRepo[];
  /**
   * Shared terminals announced into this project.
   *
   * Deleted by the cascade since terminals gained a delete verb. Before that
   * they were survivors the dialog had to disclose, which was the honest
   * answer while nothing in the product could remove one.
   */
  terminals: CascadeTerminal[];
};

/**
 * One shared-terminal announce a cascade would delete.
 *
 * Addressed by `(ownerPubkey, sessionId)` rather than by event id: the
 * announce is a replaceable head, so the coordinate is what a tombstone
 * names and what stays correct if the head is republished between the count
 * and the delete.
 */
export type CascadeTerminal = {
  sessionId: string;
  ownerPubkey: string;
  title: string;
};

/**
 * One repository a cascade would delete.
 *
 * Addressed by `(ownerPubkey, repoId)` because the tombstone names the
 * `30617:<owner>:<id>` coordinate, not an event id — the announcement is a
 * replaceable head and the coordinate stays correct if it is republished
 * between the count and the delete.
 */
export type CascadeRepo = {
  repoAddress: string;
  ownerPubkey: string;
  repoId: string;
  name: string;
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
  /** Shared terminals the cascade deletes. */
  terminals: number;
  /** Repositories the repo tick would delete. */
  repos: number;
  /** Repositories it would leave alone because it cannot delete them. */
  foreignRepos: number;
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

/**
 * Split the repositories bound to `project` into the ones this identity can
 * delete and the ones it cannot.
 *
 * ## Membership is two-sided, like channels
 *
 * A repository belongs to the project if the head forward-references it
 * (`repoAddrs`, the `["a", 30617:…]` tags) **or** if the repository itself
 * carries the `["project", <coordinate>]` back-reference. Matching only one
 * side is the bug that makes a cascade report "nothing to do" for a project
 * that plainly has children — the same reason
 * {@link channelBelongsToProject} checks both.
 *
 * ## Who may delete is *not* simply "events I signed"
 *
 * A kind:5 normally tombstones only the signer's own addressable events,
 * which is why foreign workflows are survivors. Repositories are different:
 * the relay's `project_owner_admits_deletion`
 * (`crates/buzz-relay/src/handlers/side_effects.rs`) lets a project **Owner**
 * delete an event they did not sign, for exactly three kinds — 30621, 30617
 * and 30623. Repositories are in that set.
 *
 * With one catch that this function exists to encode: for a repository that
 * gate resolves the governing project through `get_repo_project_ref`, the
 * **back-reference stored on the repo row** — not the head's forward `a` tag.
 * So a repository bound to the project by a forward ref alone falls back to
 * the authorship arm, and a non-creator Owner's delete would be accepted by
 * the relay, match no live row, and report success while changing nothing.
 *
 * Hence: deletable when the viewer signed it, or when the viewer is an Owner
 * **and** the repository carries the back-reference. Everything else is
 * reported.
 *
 * `selfPubkey` is compared case-insensitively; pass `null` while the identity
 * is loading, which classifies every repository as foreign — the safe
 * direction, since a foreign classification only ever withholds a delete.
 */
export function projectCascadeRepos(
  project: Pick<ProjectContainer, "address" | "repoAddrs">,
  repositories: readonly Repository[],
  selfPubkey: string | null,
  viewerIsOwner: boolean,
): { mine: CascadeRepo[]; foreign: CascadeRepo[] } {
  const self = selfPubkey?.toLowerCase() ?? null;
  const forwardRefs = new Set(project.repoAddrs);
  const mine: CascadeRepo[] = [];
  const foreign: CascadeRepo[] = [];
  const seen = new Set<string>();
  for (const repository of repositories) {
    if (seen.has(repository.repoAddress)) continue;
    const ref = repository.projectRef;
    const backReferenced =
      ref !== null && (normalizeProjectRef(ref) ?? ref) === project.address;
    if (!backReferenced && !forwardRefs.has(repository.repoAddress)) continue;
    seen.add(repository.repoAddress);
    const entry: CascadeRepo = {
      repoAddress: repository.repoAddress,
      ownerPubkey: repository.owner.toLowerCase(),
      repoId: repository.dtag,
      name: repository.name || repository.dtag,
    };
    const signedByMe = self !== null && entry.ownerPubkey === self;
    if (signedByMe || (viewerIsOwner && backReferenced)) mine.push(entry);
    else foreign.push(entry);
  }
  const byAddress = (a: CascadeRepo, b: CascadeRepo) =>
    a.repoAddress.localeCompare(b.repoAddress);
  return { mine: mine.sort(byAddress), foreign: foreign.sort(byAddress) };
}

/**
 * Parse the shared-terminal announces a cascade would delete.
 *
 * The relay's `#a` filter already scopes the fetch to this project, so no
 * client-side matching is needed the way it is for channels. What is needed
 * is the coordinate each tombstone will name: the announce's own `d` tag and
 * the pubkey that signed it.
 *
 * Sorted and de-duplicated so the count under the checkbox and the deletion
 * that follows it describe the same set, in the same order.
 */
export function cascadeTerminalsFromEvents(
  events: readonly RelayEvent[],
): CascadeTerminal[] {
  const seen = new Map<string, CascadeTerminal>();
  for (const event of events) {
    const sessionId = event.tags.find((tag) => tag[0] === "d")?.[1];
    if (!sessionId) continue;
    const ownerPubkey = event.pubkey.toLowerCase();
    const key = `${ownerPubkey}:${sessionId}`;
    if (seen.has(key)) continue;
    seen.set(key, {
      sessionId,
      ownerPubkey,
      title: event.tags.find((tag) => tag[0] === "title")?.[1] ?? "",
    });
  }
  return [...seen.values()].sort((a, b) =>
    `${a.ownerPubkey}:${a.sessionId}`.localeCompare(
      `${b.ownerPubkey}:${b.sessionId}`,
    ),
  );
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
    terminals: targets.terminals.length,
    repos: targets.repos.length,
    foreignRepos: targets.foreignRepos.length,
    // Only what the *main* tick deletes. Foreign workflows are deliberately
    // out: counting them here would arm "delete everything" for a project
    // whose only children the caller cannot touch. Repositories are out for
    // a different reason — they are armed by their own checkbox, so a
    // project whose only children are repositories must still read as
    // "nothing to delete" under the first one.
    total:
      targets.channels.length +
      targets.workflows.length +
      targets.terminals.length,
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
  push(counts.terminals, "shared terminal", "shared terminals");
  return parts.join(", ");
}

/**
 * The inventory under the repositories checkbox, e.g. `tank-loop,
 * tank-loop-beekeeper-agents`. Names them rather than counting them: a
 * repository is the one child here whose loss is measured in somebody's
 * commits, so "2 repositories" is not enough to decide on.
 */
export function describeProjectCascadeRepos(
  repos: readonly CascadeRepo[],
): string {
  return repos.map((repo) => repo.name).join(", ");
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
  if (counts.foreignRepos > 0) {
    notes.push(
      counts.foreignRepos === 1
        ? "1 repository in this project cannot be deleted by you — you did not announce it, and it is not linked to the project in the way the relay requires before a project owner may remove somebody else's repository."
        : `${counts.foreignRepos} repositories in this project cannot be deleted by you — you did not announce them, and they are not linked to the project in the way the relay requires before a project owner may remove somebody else's repository.`,
    );
  }
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
