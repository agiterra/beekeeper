import type { ProjectCodingSessionShelfEntry } from "./projectCodingSessionShelf";

/**
 * Which coding sessions a project's sidebar shows.
 *
 * Three independent axes:
 * - **members** — whose sessions. Attribution is by founder, the human who
 *   signed the session's kind 44226 genesis. A legacy session with no
 *   resolved genesis cannot be attributed; `mine` and `custom` hide it and
 *   report the count so the sidebar never silently drops work.
 * - **showClosed / showArchived** — closure facts (kind 44230). Archived
 *   implies closed, so `showClosed` is a superset gate: an archived session
 *   needs both boxes ticked.
 * - **range** — last activity (`session.lastEventAt`) inside a calendar
 *   window, `any` by default; hiding old work silently is not a default.
 */
export type ProjectSessionMemberFilter =
  | { mode: "mine" }
  | { mode: "all" }
  | { mode: "custom"; pubkeys: string[] };

export type ProjectSessionDateRange =
  | { kind: "any" }
  | { kind: "today" }
  | { kind: "yesterday" }
  | { kind: "week" }
  | { kind: "month" }
  /** Inclusive local calendar dates, `YYYY-MM-DD`; either bound optional. */
  | { kind: "custom"; from: string | null; to: string | null };

export type ProjectSessionFilter = {
  members: ProjectSessionMemberFilter;
  showClosed: boolean;
  showArchived: boolean;
  range: ProjectSessionDateRange;
};

export const DEFAULT_PROJECT_SESSION_FILTER: ProjectSessionFilter = {
  members: { mode: "mine" },
  showClosed: true,
  showArchived: false,
  range: { kind: "any" },
};

/** How many session rows a page shows before "Show more". */
export const PROJECT_SESSION_PAGE_SIZE = 10;

export type ProjectSessionFilterResult<
  T extends Pick<
    ProjectCodingSessionShelfEntry,
    "founderPubkey" | "isArchived" | "isClosed" | "pending" | "session"
  >,
> = {
  shown: T[];
  /** Sessions hidden only because their founder is unknown. */
  hiddenUnattributed: number;
  /** Sessions hidden by the closed/archived boxes or the date range. */
  hiddenByState: number;
};

const DATE_REGEX = /^\d{4}-\d{2}-\d{2}$/;

function parseMembers(value: unknown): ProjectSessionMemberFilter {
  if (!value || typeof value !== "object") return { mode: "mine" };
  const mode = (value as { mode?: unknown }).mode;
  if (mode === "all") return { mode: "all" };
  if (mode === "mine") return { mode: "mine" };
  if (mode === "custom") {
    const raw = (value as { pubkeys?: unknown }).pubkeys;
    const pubkeys = Array.isArray(raw)
      ? [
          ...new Set(
            raw
              .filter((entry): entry is string => typeof entry === "string")
              .map((entry) => entry.toLowerCase()),
          ),
        ]
      : [];
    return { mode: "custom", pubkeys };
  }
  return { mode: "mine" };
}

function parseRange(value: unknown): ProjectSessionDateRange {
  if (!value || typeof value !== "object") return { kind: "any" };
  const kind = (value as { kind?: unknown }).kind;
  switch (kind) {
    case "today":
    case "yesterday":
    case "week":
    case "month":
      return { kind };
    case "custom": {
      const from = (value as { from?: unknown }).from;
      const to = (value as { to?: unknown }).to;
      return {
        kind: "custom",
        from: typeof from === "string" && DATE_REGEX.test(from) ? from : null,
        to: typeof to === "string" && DATE_REGEX.test(to) ? to : null,
      };
    }
    default:
      return { kind: "any" };
  }
}

/**
 * Parse a persisted value of unknown vintage; anything odd is the default.
 * The first shape stored was the bare member filter (`{ mode }`); it reads
 * as that axis with the other axes defaulted.
 */
export function parseProjectSessionFilter(
  value: unknown,
): ProjectSessionFilter {
  if (!value || typeof value !== "object") {
    return DEFAULT_PROJECT_SESSION_FILTER;
  }
  const record = value as Record<string, unknown>;
  if ("mode" in record && !("members" in record)) {
    return { ...DEFAULT_PROJECT_SESSION_FILTER, members: parseMembers(record) };
  }
  return {
    members: parseMembers(record.members),
    showClosed:
      typeof record.showClosed === "boolean"
        ? record.showClosed
        : DEFAULT_PROJECT_SESSION_FILTER.showClosed,
    showArchived:
      typeof record.showArchived === "boolean"
        ? record.showArchived
        : DEFAULT_PROJECT_SESSION_FILTER.showArchived,
    range: parseRange(record.range),
  };
}

/** Local-calendar day boundaries, so "Today" means the user's today. */
function startOfLocalDay(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

function addDays(date: Date, days: number): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() + days);
}

function parseLocalDate(value: string): Date | null {
  const match = DATE_REGEX.test(value) ? value.split("-").map(Number) : null;
  if (!match) return null;
  const [year, month, day] = match;
  const date = new Date(year, month - 1, day);
  return Number.isNaN(date.getTime()) ? null : date;
}

/**
 * Resolve a range to `[start, end)` epoch milliseconds against `now`, or
 * null for no bound. Weeks start on Monday.
 */
export function resolveProjectSessionDateRange(
  range: ProjectSessionDateRange,
  now: Date,
): { start: number | null; end: number | null } {
  const today = startOfLocalDay(now);
  switch (range.kind) {
    case "any":
      return { start: null, end: null };
    case "today":
      return { start: today.getTime(), end: addDays(today, 1).getTime() };
    case "yesterday":
      return { start: addDays(today, -1).getTime(), end: today.getTime() };
    case "week": {
      const weekday = (today.getDay() + 6) % 7; // Monday = 0
      const monday = addDays(today, -weekday);
      return { start: monday.getTime(), end: addDays(monday, 7).getTime() };
    }
    case "month": {
      const first = new Date(today.getFullYear(), today.getMonth(), 1);
      const next = new Date(today.getFullYear(), today.getMonth() + 1, 1);
      return { start: first.getTime(), end: next.getTime() };
    }
    case "custom": {
      const from = range.from ? parseLocalDate(range.from) : null;
      const to = range.to ? parseLocalDate(range.to) : null;
      return {
        start: from ? from.getTime() : null,
        end: to ? addDays(to, 1).getTime() : null,
      };
    }
  }
}

function entryActivityMs(
  entry: Pick<ProjectCodingSessionShelfEntry, "session">,
): number | null {
  const parsed = Date.parse(entry.session.lastEventAt);
  return Number.isNaN(parsed) ? null : parsed;
}

/**
 * Apply every axis. A pending row is an optimistic stand-in for the local
 * user's own not-yet-acknowledged create, so it always counts as "mine" and
 * is never date-filtered (it has no activity yet).
 */
export function filterProjectSessions<
  T extends Pick<
    ProjectCodingSessionShelfEntry,
    "founderPubkey" | "isArchived" | "isClosed" | "pending" | "session"
  >,
>(
  entries: readonly T[],
  filter: ProjectSessionFilter,
  currentPubkey: string | undefined,
  now: Date = new Date(),
): ProjectSessionFilterResult<T> {
  const { members } = filter;
  const allowed =
    members.mode === "all"
      ? null
      : new Set(
          members.mode === "mine"
            ? currentPubkey
              ? [currentPubkey.toLowerCase()]
              : []
            : members.pubkeys.map((pubkey) => pubkey.toLowerCase()),
        );
  const window = resolveProjectSessionDateRange(filter.range, now);
  const shown: T[] = [];
  let hiddenUnattributed = 0;
  let hiddenByState = 0;
  for (const entry of entries) {
    const pending = entry.pending === true;
    if (!pending) {
      if (entry.isArchived && !(filter.showArchived && filter.showClosed)) {
        hiddenByState += 1;
        continue;
      }
      if (entry.isClosed && !filter.showClosed) {
        hiddenByState += 1;
        continue;
      }
      if (window.start !== null || window.end !== null) {
        const at = entryActivityMs(entry);
        if (
          at === null ||
          (window.start !== null && at < window.start) ||
          (window.end !== null && at >= window.end)
        ) {
          hiddenByState += 1;
          continue;
        }
      }
    }
    if (allowed === null) {
      shown.push(entry);
      continue;
    }
    if (pending && members.mode === "mine") {
      shown.push(entry);
      continue;
    }
    if (entry.founderPubkey === null) {
      hiddenUnattributed += 1;
      continue;
    }
    if (allowed.has(entry.founderPubkey.toLowerCase())) shown.push(entry);
  }
  return { shown, hiddenUnattributed, hiddenByState };
}

/** Distinct known founders across a project's sessions, first-seen order. */
export function projectSessionFounders(
  entries: readonly Pick<ProjectCodingSessionShelfEntry, "founderPubkey">[],
): string[] {
  const seen = new Set<string>();
  for (const entry of entries) {
    if (entry.founderPubkey) seen.add(entry.founderPubkey.toLowerCase());
  }
  return [...seen];
}

export function projectSessionMemberFilterLabel(
  members: ProjectSessionMemberFilter,
): string {
  switch (members.mode) {
    case "mine":
      return "My sessions";
    case "all":
      return "All sessions";
    case "custom":
      return `Custom · ${members.pubkeys.length}`;
  }
}

export function projectSessionDateRangeLabel(
  range: ProjectSessionDateRange,
): string {
  switch (range.kind) {
    case "any":
      return "Any time";
    case "today":
      return "Today";
    case "yesterday":
      return "Yesterday";
    case "week":
      return "This week";
    case "month":
      return "This month";
    case "custom":
      return range.from || range.to
        ? `${range.from ?? "…"} – ${range.to ?? "…"}`
        : "Custom";
  }
}

/** The trigger's one-line summary of the whole filter. */
export function projectSessionFilterLabel(
  filter: ProjectSessionFilter,
): string {
  const parts = [projectSessionMemberFilterLabel(filter.members)];
  if (filter.range.kind !== "any") {
    parts.push(projectSessionDateRangeLabel(filter.range));
  }
  return parts.join(" · ");
}

/** The disclosure line for sessions the current filter cannot attribute. */
export function projectSessionUnattributedNote(count: number): string | null {
  if (count <= 0) return null;
  return `${count} ${count === 1 ? "session" : "sessions"} without a known initiator ${count === 1 ? "is" : "are"} hidden — choose All sessions to see ${count === 1 ? "it" : "them"}.`;
}
