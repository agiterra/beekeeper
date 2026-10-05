/**
 * What a surface badge says, from signed facts and this device's own read
 * marker, and nothing else (SV-22, Wave B DB5–DB8).
 *
 * A badge counts only what is happening **now** and clears when it ends.
 * Three live tones and one quiet one:
 *
 * - **activity** (primary count): something is running — a subagent, a seat
 *   on a turn, a task in progress, a gate the provider signed as started.
 * - **waiting** (amber count): a seat or the mission waits on a person's
 *   ruling (DB8). Tool permission prompts never wait in Beekeeper — the fence
 *   auto-allows them — so a ruling is the only thing that does.
 * - **attention** (destructive dot): the newest gate on the head failed, a
 *   gate's newest row naming no commit failed, or the newest verdict refuses.
 * - **neutral** (muted count): Diff's files changed since this device last
 *   looked. Not live work, so it never borrows a live tone.
 *
 * Totals ever are never a badge: five finished subagents give none.
 *
 * Every function here is pure. The Badge components feed it from the surface
 * `ctx`, a clock and a localStorage marker; the header's right-panel dot (B1)
 * feeds it the same way and asks {@link strongestCodingSessionSurfaceBadgeTone}
 * which tone to draw.
 */
import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import type {
  CodingSessionObservationGateView,
  CodingSessionRunningGate,
} from "./codingSessionObservationView";
import { CODING_SESSION_LANDING_SIGNER_UNCHECKED } from "./codingSessionLandingModel";
import type { CodingSessionTaskModel } from "./codingSessionTaskModel";
import { deriveCodingSessionObservedChanges } from "./codingSessionTranscriptModelChanges";

/** The four tones, weakest first. */
export type CodingSessionSurfaceBadgeTone =
  | "neutral"
  | "activity"
  | "waiting"
  | "attention";

/** Rank of each tone; higher wins. Attention > waiting > activity > neutral. */
export const CODING_SESSION_SURFACE_BADGE_TONE_RANK: Readonly<
  Record<CodingSessionSurfaceBadgeTone, number>
> = Object.freeze({ neutral: 0, activity: 1, waiting: 2, attention: 3 });

/** One badge: its tone, the number it draws, and the facts it states. */
export type CodingSessionSurfaceBadge = {
  tone: CodingSessionSurfaceBadgeTone;
  /** The number on the pill; `null` draws a dot (attention). Never 0. */
  count: number | null;
  /**
   * Every fact the badge stands for, one sentence each, strongest first.
   * The pill's `aria-label` is these joined, so it states the fact rather
   * than a number: "2 subagents running".
   */
  facts: readonly string[];
  /** Hover-only detail one step away (e.g. the gate-start restart note). */
  detail: string | null;
};

/** The pill's accessible name: every fact, in order. */
export function codingSessionSurfaceBadgeLabel(
  badge: CodingSessionSurfaceBadge,
): string {
  return badge.facts.join(". ");
}

/**
 * The strongest tone among some badges, or `null` when none is drawn.
 *
 * Exported for the header's right-panel toggle (B1): its dot takes the
 * strongest tone among the badges of surfaces not on screen.
 */
export function strongestCodingSessionSurfaceBadgeTone(
  badges: Iterable<Pick<CodingSessionSurfaceBadge, "tone"> | null | undefined>,
): CodingSessionSurfaceBadgeTone | null {
  let strongest: CodingSessionSurfaceBadgeTone | null = null;
  for (const badge of badges) {
    if (!badge) continue;
    if (
      strongest === null ||
      CODING_SESSION_SURFACE_BADGE_TONE_RANK[badge.tone] >
        CODING_SESSION_SURFACE_BADGE_TONE_RANK[strongest]
    ) {
      strongest = badge.tone;
    }
  }
  return strongest;
}

function plural(count: number, one: string, many: string): string {
  return `${count} ${count === 1 ? one : many}`;
}

// ---------------------------------------------------------------------------
// Rulings (DB8)
// ---------------------------------------------------------------------------

/** One open ruling a badge can name: who owes it. */
export type CodingSessionSurfaceOpenRuling = {
  /** Named as the roster names them (`you`, `Brian`); null when unresolved. */
  holderLabel: string | null;
};

/**
 * One row of the team fold's `decisions[]` — the fields a badge reads.
 * `answerId` is null exactly while the request stands open.
 */
export type CodingSessionSurfaceDecisionRow = {
  readonly requestId: string;
  /** Exactly `"founder"`, or a 64-hex actor pubkey. */
  readonly heldOn: string;
  readonly answerId: string | null;
};

/**
 * The open rulings: every kind-44244 `decision.request` the team fold paired
 * with no `decision.answer` (DB8) — the one thing in a mission that waits on
 * a person.
 *
 * Read from the fold's own `decisions[]`, which is uncapped, never from the
 * open-holds list: an open **report** is owed a verdict that a lead or a
 * verifier may sign, so it is not a person's ruling, and the holds list is
 * capped at its 50 oldest entries, so a ruling past the cap would vanish.
 * `null` decisions (not read, no genesis) give none: unknown is not
 * "waiting".
 */
export function codingSessionSurfaceOpenRulings(
  decisions: readonly CodingSessionSurfaceDecisionRow[] | null,
  nameHolder: (heldOn: string) => string | null,
): CodingSessionSurfaceOpenRuling[] {
  if (decisions === null) return [];
  const seen = new Set<string>();
  const rulings: CodingSessionSurfaceOpenRuling[] = [];
  for (const decision of decisions) {
    if (decision.answerId !== null || seen.has(decision.requestId)) continue;
    seen.add(decision.requestId);
    rulings.push({ holderLabel: nameHolder(decision.heldOn) });
  }
  return rulings;
}

/** "Waiting on a ruling from you" / "Waiting on 2 rulings from you and Ira". */
export function codingSessionSurfaceRulingFact(
  rulings: readonly CodingSessionSurfaceOpenRuling[],
): string | null {
  if (rulings.length === 0) return null;
  const holders = [
    ...new Set(
      rulings.map((ruling) => ruling.holderLabel ?? "someone not resolved"),
    ),
  ];
  const from =
    holders.length === 1
      ? holders[0]
      : `${holders.slice(0, -1).join(", ")} and ${holders.at(-1)}`;
  return rulings.length === 1
    ? `Waiting on a ruling from ${from}`
    : `Waiting on ${rulings.length} rulings from ${from}`;
}

// ---------------------------------------------------------------------------
// Agents
// ---------------------------------------------------------------------------

/**
 * Agents: running subagents, plus seats on a turn, turning amber while a
 * ruling holds a seat.
 *
 * `workingSeats` counts executions on a turn (`Working`); `startingSeats`
 * counts those the provider signed `starting` and that are not on a turn yet.
 * They are separate facts because a seat coming up is not a seat working
 * (SV-43): it is activity — live and stoppable — so it counts, under its own
 * word. The single layout passes 0 for both: its one execution is the
 * session itself, whose status the header already shows, so counting it
 * here would read "1" over the very thing the person is looking at (lane
 * B3's decision).
 */
export function deriveCodingSessionAgentsBadge(input: {
  runningSubagents: number;
  workingSeats: number;
  /** Absent means none. */
  startingSeats?: number;
  rulings: readonly CodingSessionSurfaceOpenRuling[];
}): CodingSessionSurfaceBadge | null {
  const facts: string[] = [];
  const startingSeats = input.startingSeats ?? 0;
  const ruling = codingSessionSurfaceRulingFact(input.rulings);
  if (ruling) facts.push(ruling);
  if (input.workingSeats > 0) {
    facts.push(`${plural(input.workingSeats, "seat", "seats")} working`);
  }
  if (startingSeats > 0) {
    facts.push(`${plural(startingSeats, "seat", "seats")} starting`);
  }
  if (input.runningSubagents > 0) {
    facts.push(
      `${plural(input.runningSubagents, "subagent", "subagents")} running`,
    );
  }
  if (facts.length === 0) return null;
  if (ruling) {
    return {
      tone: "waiting",
      count: input.rulings.length,
      facts,
      detail: null,
    };
  }
  return {
    tone: "activity",
    count: input.runningSubagents + input.workingSeats + startingSeats,
    facts,
    detail: null,
  };
}

// ---------------------------------------------------------------------------
// Plan
// ---------------------------------------------------------------------------

/**
 * Plan: tasks in progress — only while the session is working.
 *
 * A plan an idle session left with a task `in_progress` is a snapshot, not
 * work happening now; its panel still shows it, its badge does not.
 */
export function deriveCodingSessionPlanBadge(input: {
  taskModel: CodingSessionTaskModel | null;
  working: boolean;
}): CodingSessionSurfaceBadge | null {
  if (!input.working || input.taskModel === null) return null;
  const inProgress = input.taskModel.tasks.filter(
    (task) => task.status === "in_progress",
  ).length;
  if (inProgress === 0) return null;
  return {
    tone: "activity",
    count: inProgress,
    facts: [`${plural(inProgress, "task", "tasks")} in progress`],
    detail: null,
  };
}

// ---------------------------------------------------------------------------
// Landing
// ---------------------------------------------------------------------------

/**
 * A verdict the Landing surface's own model found refusing, when it shares
 * one. The contract with lane B2: if the Landing definition's `readExtension`
 * returns `{ refusingVerdict: { label } | null }`, Landing's badge reads it
 * from `ctx.extensions.landing`. Absent means "not shared", never "approved".
 */
export type CodingSessionSurfaceLandingBadgeExtension = {
  refusingVerdict: { label: string } | null;
};

/** Read B2's extension defensively: anything else is "not shared". */
export function readCodingSessionLandingBadgeExtension(
  value: unknown,
): { label: string } | null {
  if (typeof value !== "object" || value === null) return null;
  const verdict = (value as { refusingVerdict?: unknown }).refusingVerdict;
  if (typeof verdict !== "object" || verdict === null) return null;
  const label = (verdict as { label?: unknown }).label;
  return typeof label === "string" && label.trim().length > 0
    ? { label: label.trim() }
    : null;
}

/**
 * The failed gates on the head: the commit the newest gate row names.
 *
 * "Newest" is by the relay-signed `created_at` of each row's source event —
 * the only signed time a row has. Rows naming no commit are evidence about no
 * commit, so they never decide the head and never sit on it. Of the rows on
 * the head, a gate counts as failed when its newest row (per author and gate
 * name, which the fold already reduced to one) says `failed`.
 */
export function codingSessionFailedGatesOnHead(
  gates: readonly CodingSessionObservationGateView[],
  signedAt: ReadonlyMap<string, number> | null,
): CodingSessionObservationGateView[] {
  const at = (gate: CodingSessionObservationGateView) =>
    signedAt?.get(gate.sourceEventId) ?? Number.NEGATIVE_INFINITY;
  const named = gates.filter((gate) => gate.commitSha !== null);
  if (named.length === 0) return [];
  const newest = named.reduce((left, right) =>
    at(right) > at(left) ? right : left,
  );
  return named.filter(
    (gate) => gate.commitSha === newest.commitSha && gate.outcome === "failed",
  );
}

/**
 * The failed gates that name no commit: of the rows with `commitSha` null,
 * the newest per (author, source, gate) — the Landing panel's own key and
 * rule, so a newer commitless pass for the same gate clears an older
 * failure — when that newest row says `failed`.
 *
 * Observed gate rows are signed with no head today, so this is where a
 * provider-observed failure lives. The Landing panel shows it as
 * `failed (no commit named)`; the badge must not stay silent about it.
 * "Newest" is the relay-signed `created_at`; on a tie the first row read
 * holds, as in the panel.
 */
export function codingSessionFailedCommitlessGates(
  gates: readonly CodingSessionObservationGateView[],
  signedAt: ReadonlyMap<string, number> | null,
): CodingSessionObservationGateView[] {
  const at = (gate: CodingSessionObservationGateView) =>
    signedAt?.get(gate.sourceEventId) ?? Number.NEGATIVE_INFINITY;
  const newest = new Map<string, CodingSessionObservationGateView>();
  for (const gate of gates) {
    if (gate.commitSha !== null) continue;
    const key = `${gate.authorPubkey}\u0000${gate.source}\u0000${gate.gate}`;
    const held = newest.get(key);
    if (held === undefined || at(gate) > at(held)) newest.set(key, gate);
  }
  return [...newest.values()].filter((gate) => gate.outcome === "failed");
}

/**
 * The running-gate facts, one per watcher: `Gate running: just ci, watched
 * by {provider}`. A start names the provider instance that watched it — never
 * the seat — so that is who the fact attributes it to.
 *
 * When the fold could not check the signer against the session's providers
 * (`provenanceChecked` false), the `observed` word is the signer's own claim:
 * the fact says `signed as observed by {name}`, as the Landing panel does.
 */
function runningGateFacts(
  running: readonly CodingSessionRunningGate[],
  nameWatcher: (pubkey: string) => string,
  provenanceChecked: boolean,
): string[] {
  const byWatcher = new Map<string, Set<string>>();
  for (const gate of running) {
    const watcher = nameWatcher(gate.authorPubkey);
    const gates = byWatcher.get(watcher) ?? new Set<string>();
    gates.add(gate.gate);
    byWatcher.set(watcher, gates);
  }
  // One read feeds every gate, so one note serves them all.
  const notLive = running.find((gate) => gate.notLive)?.notLive ?? null;
  return [...byWatcher].map(
    ([watcher, gates]) =>
      `Gate running: ${[...gates].join(", ")}, ${
        provenanceChecked ? "watched by" : "signed as observed by"
      } ${watcher}${notLive === null ? "" : ` (${notLive.short})`}`,
  );
}

/**
 * The hover detail for running gates: when each began, said as the clock it
 * carries — the provider's when the fold checked the signer, else only the
 * signer's, with the panel's unchecked sentence — then the restart note.
 */
function runningGateDetail(
  running: readonly CodingSessionRunningGate[],
  formatTime: (ms: number) => string,
  note: string,
  provenanceChecked: boolean,
): string {
  const clock = provenanceChecked
    ? "the provider's clock"
    : "the signer's clock";
  const started = running.map(
    (gate) =>
      `${gate.gate}: started ${formatTime(gate.startedAtMs)} by ${clock}`,
  );
  const unchecked = provenanceChecked
    ? ""
    : ` ${CODING_SESSION_LANDING_SIGNER_UNCHECKED}`;
  const notLive = running.find((gate) => gate.notLive)?.notLive ?? null;
  return `${started.join(". ")}.${unchecked}\n${note}${
    notLive === null ? "" : `\n${notLive.sentence}`
  }`;
}

/**
 * Landing: attention when the head's newest gate failed, a gate's newest
 * commitless row failed, or the newest verdict refuses; waiting on a ruling; and, from SV-41, a gate the provider
 * signed as started and has not closed — never one gone stale.
 */
export function deriveCodingSessionLandingBadge(input: {
  failedGates: readonly CodingSessionObservationGateView[];
  /**
   * Failed gates whose newest row names no commit
   * ({@link codingSessionFailedCommitlessGates}). Absent means none.
   */
  failedCommitlessGates?: readonly CodingSessionObservationGateView[];
  refusingVerdict: { label: string } | null;
  rulings: readonly CodingSessionSurfaceOpenRuling[];
  runningGates: readonly CodingSessionRunningGate[];
  /** Hover detail for a running gate (the restart disclosure). */
  runningGateNote: string;
  /** Names the provider instance that signed a start. */
  nameWatcher: (pubkey: string) => string;
  /**
   * `fold.provenanceChecked`: whether the fold checked each `observed` start
   * against the session's providers. Absent means checked (no fold read
   * claims nothing either way, as in the Landing panel).
   */
  provenanceChecked?: boolean;
  /** Clock time for a provider-clock instant, e.g. `14:02`. */
  formatTime: (ms: number) => string;
}): CodingSessionSurfaceBadge | null {
  const facts: string[] = [];
  for (const gate of input.failedGates) {
    facts.push(
      `Gate failed: ${gate.gate}${
        gate.commitShortSha ? ` on ${gate.commitShortSha}` : ""
      } (${gate.source})`,
    );
  }
  for (const gate of input.failedCommitlessGates ?? []) {
    facts.push(`Gate failed: ${gate.gate} (no commit named, ${gate.source})`);
  }
  if (input.refusingVerdict) {
    facts.push(`Verdict refuses: ${input.refusingVerdict.label}`);
  }
  const attention = facts.length > 0;
  const ruling = codingSessionSurfaceRulingFact(input.rulings);
  if (ruling) facts.push(ruling);
  const running = input.runningGates.filter((gate) => !gate.stale);
  const provenanceChecked = input.provenanceChecked ?? true;
  facts.push(
    ...runningGateFacts(running, input.nameWatcher, provenanceChecked),
  );
  if (facts.length === 0) return null;
  const detail =
    running.length > 0
      ? runningGateDetail(
          running,
          input.formatTime,
          input.runningGateNote,
          provenanceChecked,
        )
      : null;
  if (attention) return { tone: "attention", count: null, facts, detail };
  if (ruling) {
    return { tone: "waiting", count: input.rulings.length, facts, detail };
  }
  return { tone: "activity", count: running.length, facts, detail };
}

// ---------------------------------------------------------------------------
// Diff
// ---------------------------------------------------------------------------

/** A file's newest observed edit: the item that made it, and when. */
export type CodingSessionNewestFileEdit = {
  /** The transcript item id of the newest completed edit. */
  editId: string;
  /** When it completed, by the producer's clock; null when unreadable. */
  atMs: number | null;
};

/**
 * Each named file's newest completed edit.
 *
 * "Newest" is by the edit's own completion time (`completedAt`, else its
 * timestamp), with transcript order breaking ties and deciding whenever
 * either edit is undated. Time, not position, decides because the umbrella
 * hands this every execution's items concatenated in umbrella order: seat
 * A's fresh edit to a file must win over seat B's older one even though B's
 * items come later in the array.
 *
 * Uses the observed-changes fold item by item, so "which item is an edit"
 * and "which path it names" are exactly the Diff rail's rules — an edit that
 * names no file is not here, as it is not a row there.
 */
export function codingSessionNewestEditPerFile(
  transcript: readonly TranscriptItem[],
): Map<string, CodingSessionNewestFileEdit> {
  const newest = new Map<string, CodingSessionNewestFileEdit>();
  for (const item of transcript) {
    if (item.type !== "tool" || item.status !== "completed" || item.isError) {
      continue;
    }
    const [file] = deriveCodingSessionObservedChanges([item]).files;
    if (!file) continue;
    const parsed = Date.parse(item.completedAt ?? item.timestamp);
    const atMs = Number.isFinite(parsed) ? parsed : null;
    const current = newest.get(file.path);
    if (
      current !== undefined &&
      current.atMs !== null &&
      atMs !== null &&
      atMs < current.atMs
    ) {
      continue;
    }
    newest.set(file.path, { editId: item.id, atMs });
  }
  return newest;
}

/**
 * This device's record of the last time it showed Diff for one session.
 *
 * `seen` holds each file's newest edit at that moment. `opened`: Diff was the
 * active surface. `baseline`: Diff was never opened here, and this is when
 * this device first showed the session — T3 badges no old completion on
 * first load, and neither does this. `atMs` is this device's clock, kept as
 * a record only: nothing compares it with an edit's time, which is the
 * producer's clock.
 */
export type CodingSessionDiffSeenMarker = {
  via: "opened" | "baseline";
  atMs: number;
  seen: Readonly<Record<string, string>>;
};

/**
 * Files whose newest edit this device has not shown: those whose newest edit
 * is absent from the marker's `seen`, or a different item than it records.
 *
 * No clock is involved. Both markers record every file's newest edit when
 * they are written, so a file first edited later is absent and counts, and
 * an old edit is recorded and does not — whatever the producer's clock says
 * against this device's.
 */
export function codingSessionUnseenDiffFiles(
  newest: ReadonlyMap<string, CodingSessionNewestFileEdit>,
  marker: CodingSessionDiffSeenMarker | null,
): string[] {
  if (marker === null) return [];
  const unseen: string[] = [];
  for (const [path, edit] of newest) {
    if (marker.seen[path] !== edit.editId) unseen.push(path);
  }
  return unseen;
}

function codingSessionDiffMarker(
  via: CodingSessionDiffSeenMarker["via"],
  newest: ReadonlyMap<string, CodingSessionNewestFileEdit>,
  nowMs: number,
): CodingSessionDiffSeenMarker {
  const seen: Record<string, string> = {};
  for (const [path, edit] of newest) seen[path] = edit.editId;
  return { via, atMs: nowMs, seen };
}

/** The marker to write while Diff is on screen: everything shown is seen. */
export function codingSessionDiffOpenedMarker(
  newest: ReadonlyMap<string, CodingSessionNewestFileEdit>,
  nowMs: number,
): CodingSessionDiffSeenMarker {
  return codingSessionDiffMarker("opened", newest, nowMs);
}

/**
 * The marker to write when this device first shows a session whose Diff it
 * never opened: every edit already there is recorded, so none counts.
 */
export function codingSessionDiffBaselineMarker(
  newest: ReadonlyMap<string, CodingSessionNewestFileEdit>,
  nowMs: number,
): CodingSessionDiffSeenMarker {
  return codingSessionDiffMarker("baseline", newest, nowMs);
}

/** Whether writing `next` over `current` would change what counts. */
export function codingSessionDiffMarkerChanges(
  current: CodingSessionDiffSeenMarker | null,
  next: CodingSessionDiffSeenMarker,
): boolean {
  if (current === null || current.via !== next.via) return true;
  const currentKeys = Object.keys(current.seen);
  const nextKeys = Object.keys(next.seen);
  return (
    currentKeys.length !== nextKeys.length ||
    nextKeys.some((path) => current.seen[path] !== next.seen[path])
  );
}

/**
 * Diff: files changed since this device last looked — a muted count, and
 * nothing while Diff is the surface on screen.
 */
export function deriveCodingSessionDiffBadge(input: {
  unseenFiles: number;
  via: CodingSessionDiffSeenMarker["via"] | null;
  onScreen: boolean;
}): CodingSessionSurfaceBadge | null {
  if (input.onScreen || input.unseenFiles === 0 || input.via === null) {
    return null;
  }
  const files = plural(input.unseenFiles, "file", "files");
  return {
    tone: "neutral",
    count: input.unseenFiles,
    facts: [
      input.via === "opened"
        ? `${files} changed since you last opened Diff on this device`
        : `${files} changed since this session was first shown on this device`,
    ],
    detail: null,
  };
}
