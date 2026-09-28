/**
 * What opening a thread marks read: the WHOLE thread, nested branches included.
 *
 * This replaces LP4 v3's open-at-level rule (only replies revealed on open were
 * marked read; a reply in a collapsed branch kept its badge until expanded).
 * In practice that left a channel unread pip nobody could clear by looking: a
 * reply nested under another reply stayed unread through every open of the
 * thread and every reload, and only the Inbox list click cleared it, because
 * that path writes the thread-wide `thread:<root>` marker (Andy, 2026-09-28;
 * the unclearable replies were an agent's replies nested under his). Opening the
 * thread now does what the Inbox click does, so both surfaces agree.
 *
 * Returns every descendant reply with its createdAt (each gets its own
 * msg:<id> marker, which the channel pip and the root summary badge read), and
 * the newest createdAt for the aggregate `thread:<root>` marker, or null when
 * the thread has no replies with a known createdAt.
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
