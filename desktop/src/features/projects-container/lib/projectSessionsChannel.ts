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
 *   1. The project channel named `<project name> sessions` wins. The name *is*
 *      the mapping — published, visible to every member, and reproducible by
 *      any client from data it already has.
 *   2. Failing that, the project channel with the most recent session activity
 *      wins, so a project that already settled on some other channel is never
 *      handed a duplicate.
 *   3. Failing that there is no sessions channel yet, and the first
 *      project-scoped create publishes one (closed, bound to the project).
 *
 * A rename leaves rule 2 holding the line: the old channel keeps the history,
 * so it keeps winning until someone deliberately moves.
 */

/** The canonical name a project's sessions channel is created with. */
export function projectSessionsChannelName(projectName: string): string {
  const base = collapseWhitespace(projectName);
  return base.length > 0 ? `${base} sessions` : "sessions";
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

export type ProjectSessionsChannelCandidate = {
  id: string;
  name: string;
};

export type ProjectSessionsChannelResolution = {
  channelId: string;
  /** Which rule picked it — surfaced so the UI can say why. */
  reason: "name" | "activity";
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
  const wanted = projectSessionsChannelName(input.projectName);
  const named = [...input.projectChannels]
    .filter((channel) => namesMatch(channel.name, wanted))
    .sort((left, right) => left.id.localeCompare(right.id))[0];
  if (named) return { channelId: named.id, reason: "name" };

  const activity = input.sessionActivityByChannel ?? new Map();
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
