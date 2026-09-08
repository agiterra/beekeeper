/**
 * Which channel a project's coding sessions live in.
 *
 * A session's signed events are `h`-scoped to exactly one channel, so a
 * project-scoped create has to name one — and the same project must keep
 * naming the same one across restarts, devices, and identities, or every
 * machine grows its own parallel history.
 *
 * The rule is a *derivation*, not a stored mapping, on purpose. The one durable
 * store nearby (`coding_session_workdir_store`) holds host-local paths and
 * nothing else; teaching it channel ids would make a private, per-machine file
 * the authority on a value every member has to agree on. Instead:
 *
 *   0. The project channel with `channel_type = "transport"` wins outright —
 *      the relay-assigned type is the identity, immune to a person naming an
 *      ordinary channel the same way. Among several transports, the one with
 *      the newest session activity wins, then the lowest id: several exist
 *      because until 2026-09-08 every project-scoped create minted a fresh
 *      one (the dialog read its candidates from a partition that keeps only
 *      stream and forum channels, so rule 0 could never fire — seven
 *      "Mobile Test sessions" transports on the dev relay, one per create).
 *      Following activity converges every device on the channel the
 *      provider already advertises in, not on an empty one nobody joined.
 *   1. Failing that (legacy transports predate the type), the project channel
 *      named `<project name> sessions` wins. The name *is* the mapping —
 *      published, visible to every member, and reproducible by any client
 *      from data it already has.
 *   2. Failing that, the project channel with the most recent session activity
 *      wins, so a project that already settled on some other channel is never
 *      handed a duplicate.
 *   3. Failing that there is no sessions channel yet, and the first
 *      project-scoped create publishes one (a hidden transport channel bound
 *      to the project).
 *
 * A rename leaves rule 2 holding the line: the old channel keeps the history,
 * so it keeps winning until someone deliberately moves.
 */

import { normalizeProjectRef } from "./projectContainerModel";

/** The canonical name a project's sessions channel is created with. */
export function projectSessionsChannelName(projectName: string): string {
  const base = collapseWhitespace(projectName);
  return base.length > 0 ? `${base} sessions` : "sessions";
}

/** Description stamped on a channel created solely to carry session events. */
export function projectSessionsChannelDescription(projectName: string): string {
  const base = collapseWhitespace(projectName);
  return base.length > 0 ? `Coding sessions for ${base}.` : "Coding sessions.";
}

function collapseWhitespace(value: string): string {
  return value.trim().replace(/\s+/g, " ");
}

/** Case- and whitespace-insensitive comparison, so `My  Project Sessions`
 * matches a channel published as `my project sessions`. */
function namesMatch(left: string, right: string): boolean {
  return (
    collapseWhitespace(left).toLocaleLowerCase() ===
    collapseWhitespace(right).toLocaleLowerCase()
  );
}

type ProjectSessionTransportCandidate = {
  id: string;
  name: string;
  description?: string;
  channelType?: string;
};

/**
 * Remove a project's dedicated session transport from user-facing lists.
 *
 * Project-scoped creates publish a hidden transport channel because 442xx
 * events are `h`-scoped. Showing that implementation channel beside the
 * session gives two plausible doors: one opens the workspace and the other an
 * empty chat timeline. A `channel_type = "transport"` channel is always
 * hidden (the relay-assigned type is the identity). A channel carrying the
 * canonical creation-stamp description is hidden too, sessions or not — the
 * stamp is written only by this app's own fallback create against a relay
 * that predates the transport type, and waiting for session ingestion left
 * the freshly created fallback visible in the sidebar. The canonical *name*
 * alone still hides only channels that actually host sessions, so a person
 * who happens to name an ordinary chat channel `<project> sessions` keeps it.
 */
export function withoutProjectSessionTransportChannels<
  T extends ProjectSessionTransportCandidate,
>(input: {
  projectName: string;
  channels: readonly T[];
  codingSessions: readonly { channelId: string }[];
}): T[] {
  const sessionChannelIds = new Set(
    input.codingSessions.map((session) => session.channelId),
  );
  const canonicalName = projectSessionsChannelName(input.projectName);
  const canonicalDescription = projectSessionsChannelDescription(
    input.projectName,
  );
  return input.channels.filter(
    (channel) =>
      channel.channelType !== "transport" &&
      channel.description?.trim() !== canonicalDescription &&
      (!sessionChannelIds.has(channel.id) ||
        !namesMatch(channel.name, canonicalName)),
  );
}

export type ProjectSessionsChannelCandidate = {
  id: string;
  name: string;
  channelType?: string;
};

export type ProjectSessionsChannelResolution = {
  channelId: string;
  /** Which rule picked it — surfaced so the UI can say why. */
  reason: "transport" | "name" | "activity";
};

/**
 * Resolve the project's existing sessions channel, or `null` when one has to be
 * created. `sessionActivityByChannel` maps a channel id to the ISO timestamp of
 * its most recent trusted session event; channels absent from it host none.
 */
export function resolveProjectSessionsChannel(input: {
  projectName: string;
  projectChannels: readonly ProjectSessionsChannelCandidate[];
  sessionActivityByChannel?: ReadonlyMap<string, string>;
}): ProjectSessionsChannelResolution | null {
  const activity = input.sessionActivityByChannel ?? new Map();
  const transport = [...input.projectChannels]
    .filter((channel) => channel.channelType === "transport")
    .sort((left, right) => {
      const byTime = (activity.get(right.id) ?? "").localeCompare(
        activity.get(left.id) ?? "",
      );
      return byTime !== 0 ? byTime : left.id.localeCompare(right.id);
    })[0];
  if (transport) return { channelId: transport.id, reason: "transport" };

  const wanted = projectSessionsChannelName(input.projectName);
  const named = [...input.projectChannels]
    .filter((channel) => namesMatch(channel.name, wanted))
    .sort((left, right) => left.id.localeCompare(right.id))[0];
  if (named) return { channelId: named.id, reason: "name" };

  const active = [...input.projectChannels]
    .filter((channel) => activity.has(channel.id))
    .sort((left, right) => {
      const byTime = (activity.get(right.id) ?? "").localeCompare(
        activity.get(left.id) ?? "",
      );
      return byTime !== 0 ? byTime : left.id.localeCompare(right.id);
    })[0];
  return active ? { channelId: active.id, reason: "activity" } : null;
}

/**
 * Every channel a project claims, of any type — the candidate list
 * `resolveProjectSessionsChannel` must be given.
 *
 * Not `partitionChannels(...).channelsByProject`: that partition exists for
 * the sidebar and keeps only stream and forum channels, so a transport is
 * never in it and rule 0 never fires (the 2026-09-08 finding above). A
 * channel belongs here when the project's head lists its id or the channel
 * carries the project's coordinate as its back-reference — the same two
 * claims the partition honours, minus the type filter.
 */
export function projectSessionsChannelCandidates<
  T extends { id: string; projectRef?: string | null },
>(
  project: { address: string; channelIds: readonly string[] },
  channels: readonly T[],
): T[] {
  const claimed = new Set(project.channelIds);
  return channels.filter((channel) => {
    if (claimed.has(channel.id)) return true;
    const ref = channel.projectRef;
    if (!ref) return false;
    return (normalizeProjectRef(ref) ?? ref) === project.address;
  });
}

/** Newest session event per channel, from resolved shelf entries. */
export function projectSessionActivityByChannel(
  entries: readonly {
    channelId: string;
    session: { lastEventAt: string };
  }[],
): Map<string, string> {
  const activity = new Map<string, string>();
  for (const entry of entries) {
    const current = activity.get(entry.channelId);
    if (!current || current.localeCompare(entry.session.lastEventAt) < 0) {
      activity.set(entry.channelId, entry.session.lastEventAt);
    }
  }
  return activity;
}
