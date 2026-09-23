import { KIND_DELETION, KIND_PROJECT } from "@/shared/constants/kinds";

/**
 * The relay filters that make a project change reach this client live.
 *
 * Pure, and separate from the hook, because one property here is a safety
 * rule rather than a detail: the deletion filter is **always** scoped by
 * `#a`, and is omitted entirely rather than ever going out unscoped.
 *
 * ## Why a project needs its own subscription at all
 *
 * A project tombstone is a kind:5 whose only tag is
 * `["a", "30621:<owner>:<slug>"]` — it carries no `h` tag, because the relay
 * routes it entirely on that coordinate. The relay therefore publishes it on
 * the *global* topic, and the app's only live kind:5 subscriptions cannot
 * receive it: `useLiveChannelUpdates` is `#h`-scoped, and the relay's own
 * invariant is that channel-scoped subscriptions never see global events.
 * `usePersonaSync` also carries kind:5 but is scoped to your own pubkey, so a
 * teammate's tombstone never matches. Nothing else polls. Before this, the
 * only thing that refreshed another client's project list was a relay
 * reconnect.
 *
 * ## Why the deletion filter is never unscoped
 *
 * Two independent reasons, and the second is the one that matters:
 *
 * 1. kind:5 is also how every *message* deletion is published. A bare
 *    `{kinds:[5]}` would fan every message deletion in the community to
 *    every client, to keep a list that changes a few times a week.
 * 2. The relay's global fan-out is **ungated** — access filtering returns
 *    early for an event with no channel — so a bare `{kinds:[5]}` delivers
 *    tombstones, coordinate and all, for **private projects the viewer
 *    cannot read**. That is pre-existing relay behaviour, not something a
 *    client introduces, but a client has no business widening its exposure
 *    to it.
 *
 * Scoping to coordinates already on screen leaks nothing new by
 * construction, which is why an empty coordinate set yields no deletion
 * filter at all rather than an unscoped one.
 */
export type LiveProjectFilter = {
  kinds: number[];
  limit: number;
  "#a"?: string[];
};

/**
 * Build the live filters for `coordinates` — the `30621:<owner>:<d>`
 * addresses this client currently knows about.
 *
 * The head filter is unconditional: a project *created* on another client is
 * a coordinate this one has never seen, so it cannot be watched by `#a`.
 * That filter is cheap — project heads are rare — and is what makes creates,
 * renames and visibility changes propagate too.
 */
export function liveProjectFilters(
  coordinates: readonly string[],
): LiveProjectFilter[] {
  const filters: LiveProjectFilter[] = [
    // `limit: 0` — live only. History is already covered by the queries'
    // own fetch on mount and on reconnect.
    { kinds: [KIND_PROJECT], limit: 0 },
  ];
  const scoped = [...new Set(coordinates)].sort();
  if (scoped.length > 0) {
    filters.push({ kinds: [KIND_DELETION], limit: 0, "#a": scoped });
  }
  return filters;
}

/**
 * Whether `event` is a change to a project this client is watching.
 *
 * A project head always counts — including one for a coordinate not yet
 * known, which is exactly the create case. A kind:5 counts only when it
 * names a watched coordinate: the relay ORs the filters onto one REQ, so a
 * deletion for something else can arrive on the same subscription, and
 * re-fetching the whole project list for an unrelated tombstone is waste.
 */
export function isWatchedProjectEvent(
  event: { kind: number; tags: string[][] },
  coordinates: ReadonlySet<string>,
): boolean {
  if (event.kind === KIND_PROJECT) return true;
  if (event.kind !== KIND_DELETION) return false;
  return event.tags.some(
    (tag) => tag[0] === "a" && tag[1] !== undefined && coordinates.has(tag[1]),
  );
}
