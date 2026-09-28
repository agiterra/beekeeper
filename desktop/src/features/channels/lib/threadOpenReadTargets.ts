/**
 * How far opening a thread advances its AGGREGATE `thread:<root>` marker.
 *
 * Andy, 2026-09-28: a reply nested under another reply left a channel pip
 * nobody could clear by looking — it survived every open of the thread and
 * every reload, and only the Inbox list click cleared it for good. That path
 * writes the thread-wide `thread:<root>` marker; the thread view did not, so
 * the pip's predicate (resolveChannelActivityFeedItemReadAt) never saw a read
 * that covered the nested reply.
 *
 * Opening the thread now writes that same aggregate marker, at the newest
 * reply in the whole thread — collapsed branches included, because the pip
 * counts those replies too. What it deliberately does NOT do is advance each
 * nested reply's own `msg:<id>`: that is per-message state the in-panel branch
 * badges and the in-thread "New" divider read, and clearing it wholesale
 * emptied both (88fb730c0, corrected in ledger 279(g)). Per-message marking
 * stays open-at-level — the replies the open revealed, and no others.
 *
 * Returns `latest`, the newest descendant `createdAt`, or null when the thread
 * has no replies with a known createdAt — in which case no aggregate marker is
 * written at all, so a first reply still arrives unread. `replies` is the walk
 * it came from, kept because the badge surfaces read the same descendant set.
 */
export function threadOpenReadTargets(
  rootId: string,
  getReplyDescendantIds: (messageId: string) => readonly string[],
  createdAtByMessageId: ReadonlyMap<string, number>,
): { replies: Array<[id: string, createdAt: number]>; latest: number | null } {
  const replies: Array<[string, number]> = [];
  let latest: number | null = null;
  for (const id of getReplyDescendantIds(rootId)) {
    const createdAt = createdAtByMessageId.get(id);
    if (createdAt === undefined) continue;
    replies.push([id, createdAt]);
    if (latest === null || createdAt > latest) latest = createdAt;
  }
  return { replies, latest };
}
