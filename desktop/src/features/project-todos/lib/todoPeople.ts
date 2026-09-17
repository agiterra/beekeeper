/**
 * Who a pubkey on a to-do row is: display name, avatar, and whether it is an
 * agent (which decides the glyph beside the avatar). Resolved from the
 * users-batch profile cache; a pubkey with no profile renders as its short
 * form rather than as nothing.
 */
import type { UserProfileSummary } from "@/shared/api/types";
import { truncatePubkey } from "@/shared/lib/pubkey";

export type TodoPerson = {
  pubkey: string;
  name: string;
  avatarUrl: string | null;
  isAgent: boolean;
};

/** Resolve one pubkey against a profiles map. */
export function todoPerson(
  pubkey: string,
  profiles: Record<string, UserProfileSummary> | undefined,
): TodoPerson {
  const profile = profiles?.[pubkey.toLowerCase()];
  return {
    pubkey,
    name: profile?.displayName || profile?.name || truncatePubkey(pubkey),
    avatarUrl: profile?.avatarUrl ?? null,
    isAgent: profile?.isAgent === true || !!profile?.ownerPubkey,
  };
}

/** Every pubkey a digest references (assignees, authors), for one batch read. */
export function todoPubkeysOf(
  lists: readonly {
    open: readonly {
      assignee: string | null;
      createdBy: string;
      completedBy: string | null;
    }[];
    completed: readonly {
      assignee: string | null;
      createdBy: string;
      completedBy: string | null;
    }[];
  }[],
): string[] {
  const set = new Set<string>();
  for (const list of lists) {
    for (const item of [...list.open, ...list.completed]) {
      if (item.assignee) set.add(item.assignee);
      set.add(item.createdBy);
      if (item.completedBy) set.add(item.completedBy);
    }
  }
  return [...set];
}

/** Today as `YYYY-MM-DD` in the viewer's local calendar. */
export function todayIso(now: Date = new Date()): string {
  const y = now.getFullYear();
  const m = String(now.getMonth() + 1).padStart(2, "0");
  const d = String(now.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

/** `true` when `due` (YYYY-MM-DD) is before today. */
export function isOverdue(due: string, today: string = todayIso()): boolean {
  return due < today;
}

/** "Oct 1", or "Oct 1, 2027" when the year is not this one. */
export function formatDue(due: string, today: string = todayIso()): string {
  const [y, m, d] = due.split("-").map(Number);
  if (!y || !m || !d) return due;
  const date = new Date(y, m - 1, d);
  const sameYear = due.slice(0, 4) === today.slice(0, 4);
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    ...(sameYear ? {} : { year: "numeric" }),
  }).format(date);
}
