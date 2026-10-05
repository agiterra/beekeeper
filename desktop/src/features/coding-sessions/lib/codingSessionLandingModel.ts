/**
 * The Landing surface's four rows (SV-24, DB4): Gate, Verdict, Land, Landed.
 *
 * T3 Code's pull-request panel reads a host's checks, review and merge state.
 * Beekeeper has no pull request (D6); the same four questions are answered
 * from signed facts and one relay read, and each row says which:
 *
 * - **Gate**: the newest kind 44246 gate row per gate for the newest head,
 *   with its source word, commit, dirty mark and command — plus every gate
 *   the provider signed as started and has not signed as ended (SV-41).
 * - **Verdict**: the newest disposition or refutation, and who signed it,
 *   from the land rule's own `newestVerdict`.
 * - **Land**: the existing control's state. It never runs git.
 * - **Landed**: whether the head is in the relay's `main`, from a bounded
 *   history read, stamped with when it was checked. A head missing from that
 *   history is "not seen in main's last N commits" — never "not landed".
 *
 * **No read never turns red.** A row this view could not read is `unknown`,
 * a muted word, and says what was not read. `attention` is reserved for a
 * signed fact that says something failed or refused
 * (`hooks/useCodingSessionMissionLand.ts:214-240` keeps the same rule).
 *
 * Pure: no React, no clock, no relay. The panel supplies every read and the
 * time formatter, so this file is the one place the wording lives and the
 * unit test pins it.
 */
import type {
  CodingSessionMissionLandModel,
  CodingSessionMissionLandUnavailableReason,
  CodingSessionLandNewestVerdict,
} from "./codingSessionMissionLand";
import type {
  CodingSessionObservationGateView,
  CodingSessionRunningGate,
} from "./codingSessionObservationView";

/** How loudly a row speaks. `unknown` is a missing read, never a failure. */
export type CodingSessionLandingTone =
  | "neutral"
  | "ok"
  | "running"
  | "attention"
  | "unknown";

/** The sentence a not-read row carries, beside its muted word. */
export const CODING_SESSION_LANDING_NOT_READ = "not read";

/** The running-gate title: a restart is not observed (SV-41 S5 pending). */
export const CODING_SESSION_LANDING_RUNNING_TITLE =
  "Observed by the provider. A provider restart is not observed: if it restarted mid-gate, this line can outlive the process that ran the gate until it goes stale.";

/**
 * What a gate start or `observed` row carries when the fold could not check
 * its signer against the session's providers (`fold.provenanceChecked`
 * false): buzz-core then accepts the word as written.
 */
export const CODING_SESSION_LANDING_SIGNER_UNCHECKED =
  "Signed as observed; this view could not check the signer against the session's providers.";

/** The Audit section's own sentence for the same unchecked fold. */
export const CODING_SESSION_LANDING_PROVENANCE_UNCHECKED_NOTE =
  "Provenance was not verified in this view: no provider instance was resolved for this session, so an `observed` word here is the author's own claim.";

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

/** The view's one 44246 read, reduced to what the Gate row needs. */
export type CodingSessionLandingGateRead =
  | { state: "not-read"; reason: string }
  | { state: "loading" }
  | { state: "error"; message: string }
  | {
      state: "read";
      rows: readonly CodingSessionObservationGateView[];
      /** Signed `created_at` (unix seconds) per event id. */
      signedAt: ReadonlyMap<string, number>;
    };

/** The land rule's answer, reduced to what Verdict and Land need. */
export type CodingSessionLandingRuleRead =
  | { state: "not-read"; reason: string }
  | { state: "asking" }
  | { state: "failed" }
  | {
      state: "read";
      land: CodingSessionMissionLandModel;
      newestVerdict: CodingSessionLandNewestVerdict | null;
    };

/** The `main` history read behind the Landed row. */
export type CodingSessionLandingMainRead =
  | { state: "not-read"; reason: string }
  | { state: "loading" }
  | { state: "error"; message: string }
  | {
      state: "read";
      /** Full commit ids, newest first, as the bounded read returned them. */
      commits: readonly string[];
      checkedAtMs: number;
      /** A later refresh failed; the commits above are the older read. */
      refreshError?: string | null;
    };

export type CodingSessionLandingModelInput = {
  gates: CodingSessionLandingGateRead;
  running: readonly CodingSessionRunningGate[];
  /**
   * `fold.provenanceChecked`: whether the fold checked each `observed`
   * signer against the session's providers. False makes every running line
   * and observed row say the signer was not checked.
   */
  provenanceChecked: boolean;
  rule: CodingSessionLandingRuleRead;
  main: CodingSessionLandingMainRead;
  /** `{Who}` for a pubkey: the surface's own resolver. */
  resolveWho: (pubkey: string) => string;
  /** Clock time for a millisecond instant, e.g. `14:02`. */
  formatTime: (ms: number) => string;
  /**
   * The session names no repository and none was inferred: Landed, and
   * Land when the rule was not asked, say there is nothing to land into
   * rather than "not read" (a rule that answered says so itself). Absent
   * means false.
   */
  noRepository?: boolean;
};

/** What Land and Landed say for a session with no repository. */
export const CODING_SESSION_LANDING_NO_REPOSITORY =
  "This session has no repository to land into.";

// ---------------------------------------------------------------------------
// Outputs
// ---------------------------------------------------------------------------

export type CodingSessionLandingHead = {
  sha: string;
  short: string;
  /** Where the head came from, in words, for the title. */
  source: "gate" | "verdict" | "land";
};

export type CodingSessionLandingRunningLine = {
  key: string;
  gate: string;
  /** `cargo test · running since 14:02 · watched by {provider}`. */
  line: string;
  /** Whose clock dated it, the restart note, and any unchecked signer. */
  title: string;
  stale: boolean;
  /** The start event, for "find it on the wire". */
  eventId: string;
};

export type CodingSessionLandingGateRow = {
  tone: CodingSessionLandingTone;
  word: string;
  sentence: string;
  /**
   * The newest gate row per (author, source, gate) for the head, failures
   * first — the fold's own key, so an observed row and a declared row about
   * one gate never shadow each other.
   */
  rows: readonly CodingSessionObservationGateView[];
  /**
   * The newest row per (author, source, gate) that names no commit, failures
   * first. Observed rows are signed with no head today, so a failed observed
   * gate lives here and must stay visible.
   */
  unnamedRows: readonly CodingSessionObservationGateView[];
  running: readonly CodingSessionLandingRunningLine[];
  /** Disclosures: rows not shown here, and why. Empty when none. */
  notes: readonly string[];
};

export type CodingSessionLandingVerdictRow = {
  tone: CodingSessionLandingTone;
  word: string;
  sentence: string;
  eventId: string | null;
};

export type CodingSessionLandingLandRow = {
  tone: CodingSessionLandingTone;
  word: string;
  /** The control's model, when the rule answered; the panel renders it. */
  land: CodingSessionMissionLandModel | null;
  /** Said instead of the control when there is no model. */
  sentence: string | null;
};

export type CodingSessionLandingLandedRow = {
  tone: CodingSessionLandingTone;
  word: string;
  sentence: string;
  /** `checked at 14:02`, or null when nothing was checked. */
  checkedAt: string | null;
};

export type CodingSessionLandingModel = {
  head: CodingSessionLandingHead | null;
  gate: CodingSessionLandingGateRow;
  verdict: CodingSessionLandingVerdictRow;
  land: CodingSessionLandingLandRow;
  landed: CodingSessionLandingLandedRow;
};

// ---------------------------------------------------------------------------
// Gate
// ---------------------------------------------------------------------------

const OUTCOME_ORDER: Readonly<
  Record<CodingSessionObservationGateView["outcome"], number>
> = { failed: 0, "not-run": 1, passed: 2 };

function signedAtOf(
  row: CodingSessionObservationGateView,
  signedAt: ReadonlyMap<string, number>,
): number {
  return signedAt.get(row.sourceEventId) ?? Number.NEGATIVE_INFINITY;
}

/**
 * The newest head a gate row names: the commit on the most recently signed
 * row that names one. A row naming no commit speaks for no head.
 */
export function codingSessionLandingNewestGateHead(
  rows: readonly CodingSessionObservationGateView[],
  signedAt: ReadonlyMap<string, number>,
): string | null {
  let best: CodingSessionObservationGateView | null = null;
  for (const row of rows) {
    if (row.commitSha === null) continue;
    if (best === null || signedAtOf(row, signedAt) > signedAtOf(best, signedAt))
      best = row;
  }
  return best?.commitSha ?? null;
}

function plural(count: number, one: string, many: string): string {
  return `${count} ${count === 1 ? one : many}`;
}

function runningLines(
  running: readonly CodingSessionRunningGate[],
  formatTime: (ms: number) => string,
  resolveWho: (pubkey: string) => string,
  provenanceChecked: boolean,
): CodingSessionLandingRunningLine[] {
  return running.map((gate) => {
    const at = formatTime(gate.startedAtMs);
    const who = resolveWho(gate.authorPubkey);
    const by = provenanceChecked
      ? `watched by ${who}`
      : `signed as observed by ${who}`;
    const clock = provenanceChecked
      ? `Started ${at} by the provider's clock.`
      : `Started ${at} by the signer's clock. ${CODING_SESSION_LANDING_SIGNER_UNCHECKED}`;
    // A snapshot says so on the line and in the title (SV-41).
    const notLive = gate.notLive ?? null;
    return {
      key: gate.key,
      gate: gate.gate,
      eventId: gate.eventId,
      stale: gate.stale,
      line: `${
        gate.stale
          ? `${gate.gate} · started ${at} · no result observed · ${by}`
          : `${gate.gate} · running since ${at} · ${by}`
      }${notLive === null ? "" : ` · ${notLive.short}`}`,
      title: `${
        gate.stale
          ? `${clock} It was signed as started and nothing has been signed since, so it reads as no result rather than running.`
          : `${clock} ${CODING_SESSION_LANDING_RUNNING_TITLE}`
      }${notLive === null ? "" : ` ${notLive.sentence}`}`,
    };
  });
}

function rowKey(row: CodingSessionObservationGateView): string {
  return `${row.authorPubkey}\u0000${row.source}\u0000${row.gate}`;
}

/** Newest row per the fold's key, failures first; counts what it dropped. */
function newestPerKey(
  rows: readonly CodingSessionObservationGateView[],
  signedAt: ReadonlyMap<string, number>,
): { rows: CodingSessionObservationGateView[]; shadowed: number } {
  const newest = new Map<string, CodingSessionObservationGateView>();
  let shadowed = 0;
  for (const row of rows) {
    const key = rowKey(row);
    const held = newest.get(key);
    if (held === undefined) {
      newest.set(key, row);
      continue;
    }
    shadowed += 1;
    if (signedAtOf(row, signedAt) > signedAtOf(held, signedAt)) {
      newest.set(key, row);
    }
  }
  return {
    shadowed,
    rows: [...newest.values()].sort(
      (left, right) =>
        OUTCOME_ORDER[left.outcome] - OUTCOME_ORDER[right.outcome] ||
        left.gate.localeCompare(right.gate) ||
        left.source.localeCompare(right.source),
    ),
  };
}

/** Distinct gate names, in first-seen order. */
function gateNames(
  rows: readonly CodingSessionObservationGateView[],
): string[] {
  return [...new Set(rows.map((row) => row.gate))];
}

function gateRow(
  read: CodingSessionLandingGateRead,
  running: readonly CodingSessionRunningGate[],
  formatTime: (ms: number) => string,
  resolveWho: (pubkey: string) => string,
  provenanceChecked: boolean,
): { row: CodingSessionLandingGateRow; head: string | null } {
  const lines = runningLines(
    running,
    formatTime,
    resolveWho,
    provenanceChecked,
  );
  const live = lines.filter((line) => !line.stale);
  if (read.state !== "read") {
    // A start the provider signed is a fact even when the rows are unread.
    const base = {
      rows: [],
      unnamedRows: [],
      running: lines,
      notes:
        !provenanceChecked && lines.length > 0
          ? [CODING_SESSION_LANDING_PROVENANCE_UNCHECKED_NOTE]
          : [],
    } as const;
    if (read.state === "loading") {
      return {
        head: null,
        row: {
          ...base,
          tone: live.length > 0 ? "running" : "unknown",
          word: live.length > 0 ? "running" : "reading",
          sentence: "Reading this session's gate rows.",
        },
      };
    }
    return {
      head: null,
      row: {
        ...base,
        tone: live.length > 0 ? "running" : "unknown",
        word: live.length > 0 ? "running" : CODING_SESSION_LANDING_NOT_READ,
        sentence:
          read.state === "not-read"
            ? read.reason
            : `Gate rows were not read: ${read.message}`,
      },
    };
  }

  const head = codingSessionLandingNewestGateHead(read.rows, read.signedAt);
  const otherHead = read.rows.filter(
    (row) => row.commitSha !== null && row.commitSha !== head,
  ).length;
  const onHead = newestPerKey(
    read.rows.filter((row) => head !== null && row.commitSha === head),
    read.signedAt,
  );
  const unnamed = newestPerKey(
    read.rows.filter((row) => row.commitSha === null),
    read.signedAt,
  );
  const rows = onHead.rows;
  const unnamedRows = unnamed.rows;
  const shadowed = onHead.shadowed + unnamed.shadowed;
  const notes: string[] = [];
  if (shadowed > 0) {
    notes.push(
      `${plural(shadowed, "older statement", "older statements")} by the same author about the same gate ${shadowed === 1 ? "is" : "are"} not listed.`,
    );
  }
  if (otherHead > 0) {
    notes.push(
      `${plural(otherHead, "gate row names", "gate rows name")} an older commit and ${otherHead === 1 ? "is" : "are"} not listed.`,
    );
  }
  if (
    !provenanceChecked &&
    (lines.length > 0 ||
      [...rows, ...unnamedRows].some((row) => row.source === "observed"))
  ) {
    notes.push(CODING_SESSION_LANDING_PROVENANCE_UNCHECKED_NOTE);
  }

  // Per gate *name*, the strongest outcome across every source: a failure
  // any author signed is not hidden by a pass another signed (the badge
  // counts it the same way).
  const short = head?.slice(0, 8) ?? null;
  const names = gateNames(rows);
  const failedNames = gateNames(rows.filter((row) => row.outcome === "failed"));
  const passedNames = names.filter((name) =>
    rows.every((row) => row.gate !== name || row.outcome === "passed"),
  );
  const dirtyPassedNames = passedNames.filter((name) =>
    rows.some((row) => row.gate === name && row.dirty === true),
  );
  const unnamedFailed = gateNames(
    unnamedRows.filter((row) => row.outcome === "failed"),
  );
  const headPart =
    head === null || names.length === 0
      ? ""
      : `; ${passedNames.length} of ${plural(names.length, "gate", "gates")} passed on ${short}`;
  const dirtyClause =
    dirtyPassedNames.length === 0
      ? ""
      : `, ${dirtyPassedNames.length} over uncommitted changes, which is not evidence about that commit`;
  let tone: CodingSessionLandingTone;
  let word: string;
  let sentence: string;
  if (failedNames.length > 0) {
    tone = "attention";
    word = "failed";
    sentence = `${failedNames.length} of ${plural(names.length, "gate", "gates")} failed on ${short}.`;
    if (unnamedFailed.length > 0) {
      sentence += ` ${unnamedFailed.join(", ")} also failed in a row that names no commit.`;
    }
  } else if (unnamedFailed.length > 0) {
    tone = "attention";
    word = "failed (no commit named)";
    sentence = `${unnamedFailed.join(", ")} failed in ${unnamedFailed.length === 1 ? "a row" : "rows"} that ${unnamedFailed.length === 1 ? "names" : "name"} no commit${headPart}${dirtyClause}.`;
  } else if (live.length > 0) {
    tone = "running";
    word = "running";
    sentence =
      names.length === 0
        ? `${plural(live.length, "gate is", "gates are")} running. No finished gate row names a commit yet.`
        : `${plural(live.length, "gate is", "gates are")} running; ${passedNames.length} of ${plural(names.length, "finished gate", "finished gates")} passed on ${short}${dirtyClause}.`;
  } else if (head === null) {
    tone = "neutral";
    word = read.rows.length === 0 ? "no gate rows" : "no commit named";
    sentence =
      read.rows.length === 0
        ? "No gate row has been signed in this session yet."
        : "No gate row names a commit, so none can speak for a head.";
  } else if (passedNames.length === names.length) {
    tone = dirtyPassedNames.length > 0 ? "neutral" : "ok";
    word = dirtyPassedNames.length > 0 ? "passed (dirty tree)" : "passed";
    sentence = `${plural(names.length, "gate", "gates")} passed on ${short}${dirtyClause}.`;
  } else {
    tone = "neutral";
    word = "not all run";
    sentence = `${passedNames.length} of ${plural(names.length, "gate", "gates")} passed on ${short}${dirtyClause}; the rest did not run.`;
  }
  return {
    head,
    row: { tone, word, sentence, rows, unnamedRows, running: lines, notes },
  };
}

// ---------------------------------------------------------------------------
// Verdict
// ---------------------------------------------------------------------------

/** How a signed decision word reads: clears, refuses, or neither. */
export function codingSessionLandingVerdictClass(
  decision: string,
): "clears" | "refuses" | "other" {
  switch (decision) {
    case "approve":
    case "approve-with-notes":
    case "not-refuted":
      return "clears";
    case "reject":
    case "changes-requested":
    case "refuted":
      return "refuses";
    default:
      return "other";
  }
}

function verdictRow(
  rule: CodingSessionLandingRuleRead,
  head: string | null,
  resolveWho: (pubkey: string) => string,
): CodingSessionLandingVerdictRow {
  if (rule.state === "not-read") {
    return {
      tone: "unknown",
      word: CODING_SESSION_LANDING_NOT_READ,
      sentence: rule.reason,
      eventId: null,
    };
  }
  if (rule.state === "asking") {
    return {
      tone: "unknown",
      word: "reading",
      sentence: "Asking the land rule for this mission's newest verdict.",
      eventId: null,
    };
  }
  if (rule.state === "failed") {
    return {
      tone: "unknown",
      word: CODING_SESSION_LANDING_NOT_READ,
      sentence:
        "The land rule could not be asked from this view, so its newest verdict was not read.",
      eventId: null,
    };
  }
  const verdict = rule.newestVerdict;
  if (verdict === null) {
    return {
      tone: "neutral",
      word: "none",
      sentence: "No verdict has been signed in this mission yet.",
      eventId: null,
    };
  }
  const kind = codingSessionLandingVerdictClass(verdict.decision);
  const over =
    verdict.headSha === null
      ? `over report ${verdict.reportEventId.slice(0, 8)}, which names no commit`
      : `over report ${verdict.reportEventId.slice(0, 8)} on ${verdict.headSha.slice(0, 8)}`;
  const elsewhere =
    verdict.headSha !== null &&
    head !== null &&
    verdict.headSha.toLowerCase() !== head.toLowerCase()
      ? ` That is not the newest gated commit, ${head.slice(0, 8)}.`
      : "";
  return {
    tone:
      kind === "refuses" ? "attention" : kind === "clears" ? "ok" : "neutral",
    word: verdict.decision,
    sentence: `${verdict.decision} by ${resolveWho(verdict.authorPubkey)}, ${over}.${elsewhere}`,
    eventId: verdict.eventId,
  };
}

// ---------------------------------------------------------------------------
// Land
// ---------------------------------------------------------------------------

const NO_GATE_ROWS_READ_MARK = "This view read no gate rows";

function landRow(
  rule: CodingSessionLandingRuleRead,
): CodingSessionLandingLandRow {
  if (rule.state === "not-read") {
    return {
      tone: "unknown",
      word: CODING_SESSION_LANDING_NOT_READ,
      land: null,
      sentence: rule.reason,
    };
  }
  if (rule.state === "asking") {
    return {
      tone: "unknown",
      word: "reading",
      land: null,
      sentence: "Asking the push path's rule whether this commit may land.",
    };
  }
  if (rule.state === "failed") {
    return {
      tone: "unknown",
      word: CODING_SESSION_LANDING_NOT_READ,
      land: null,
      sentence:
        "The push path's rule could not be asked from this view, so nothing here can say whether this commit may land.",
    };
  }
  const { land } = rule;
  switch (land.state) {
    case "ready":
      return { tone: "ok", word: "ready", land, sentence: null };
    case "refused":
      // A refusal over gate rows this view never read is a fact about the
      // read, not the mission, and does not turn red.
      return {
        tone: land.sentence?.includes(NO_GATE_ROWS_READ_MARK)
          ? "unknown"
          : "attention",
        word: "refused",
        land,
        sentence: null,
      };
    case "ungoverned":
      return { tone: "neutral", word: "not governed", land, sentence: null };
    default:
      return {
        tone: "unknown",
        word: CODING_SESSION_LANDING_NOT_READ,
        land,
        sentence: null,
      };
  }
}

/** Why the land rule was not asked, as the Land and Verdict rows say it. */
export function codingSessionLandingRuleUnavailableReason(
  reason: CodingSessionMissionLandUnavailableReason,
): CodingSessionLandingRuleRead {
  return reason === "boundary-failed"
    ? { state: "failed" }
    : {
        state: "not-read",
        reason:
          "Nothing to ask yet: this view has not resolved who is asking, or holds no mission evidence.",
      };
}

// ---------------------------------------------------------------------------
// Landed
// ---------------------------------------------------------------------------

function landedRow(
  main: CodingSessionLandingMainRead,
  head: string | null,
  formatTime: (ms: number) => string,
): CodingSessionLandingLandedRow {
  if (head === null) {
    return {
      tone: "neutral",
      word: "no head",
      sentence:
        "No gate row, verdict or land answer names a commit, so there is nothing to look for in main.",
      checkedAt: null,
    };
  }
  if (main.state === "not-read") {
    return {
      tone: "unknown",
      word: CODING_SESSION_LANDING_NOT_READ,
      sentence: main.reason,
      checkedAt: null,
    };
  }
  if (main.state === "loading") {
    return {
      tone: "unknown",
      word: "reading",
      sentence: "Reading main's recent history from the relay.",
      checkedAt: null,
    };
  }
  if (main.state === "error") {
    return {
      tone: "unknown",
      word: CODING_SESSION_LANDING_NOT_READ,
      sentence: `main's history was not read: ${main.message}`,
      checkedAt: null,
    };
  }
  // A refresh that failed over an older read keeps the older answer and
  // says so: the stamp is the old read's, and the person must not believe
  // they re-checked.
  const refreshNote =
    main.refreshError == null
      ? ""
      : ` Checking again failed (${main.refreshError}); this is the read from ${formatTime(main.checkedAtMs)}.`;
  if (main.commits.length === 0) {
    return {
      tone: "unknown",
      word: "empty history",
      sentence: `main's history came back empty.${refreshNote}`,
      checkedAt: `checked at ${formatTime(main.checkedAtMs)}`,
    };
  }
  const needle = head.toLowerCase();
  const index = main.commits.findIndex((commit) =>
    commit.toLowerCase().startsWith(needle),
  );
  const checkedAt = `checked at ${formatTime(main.checkedAtMs)}`;
  if (index >= 0) {
    return {
      tone: "ok",
      word: "on main",
      sentence:
        (index === 0
          ? `${head.slice(0, 8)} is main's newest commit.`
          : `${head.slice(0, 8)} is on main, ${plural(index, "commit", "commits")} behind its tip.`) +
        refreshNote,
      checkedAt,
    };
  }
  return {
    tone: "neutral",
    word: "not seen",
    sentence: `${head.slice(0, 8)} is not seen in main's last ${plural(main.commits.length, "commit", "commits")}.${refreshNote}`,
    checkedAt,
  };
}

// ---------------------------------------------------------------------------
// The whole surface
// ---------------------------------------------------------------------------

/**
 * The head the Landed row looks for: the newest gated commit, else the
 * newest verdict's report commit, else the commit the land rule admitted.
 */
function chooseHead(
  gateHead: string | null,
  rule: CodingSessionLandingRuleRead,
): CodingSessionLandingHead | null {
  if (gateHead !== null) {
    return { sha: gateHead, short: gateHead.slice(0, 8), source: "gate" };
  }
  if (rule.state !== "read") return null;
  const verdictHead = rule.newestVerdict?.headSha ?? null;
  if (verdictHead !== null) {
    return {
      sha: verdictHead,
      short: verdictHead.slice(0, 8),
      source: "verdict",
    };
  }
  const landHead = rule.land.headSha;
  return landHead === null
    ? null
    : { sha: landHead, short: landHead.slice(0, 8), source: "land" };
}

export function codingSessionLandingModel(
  input: CodingSessionLandingModelInput,
): CodingSessionLandingModel {
  const gate = gateRow(
    input.gates,
    input.running,
    input.formatTime,
    input.resolveWho,
    input.provenanceChecked,
  );
  const head = chooseHead(gate.head, input.rule);
  const noRepository = input.noRepository === true;
  return {
    head,
    gate: gate.row,
    verdict: verdictRow(input.rule, head?.sha ?? null, input.resolveWho),
    land:
      noRepository && input.rule.state === "not-read"
        ? {
            tone: "neutral",
            word: "no repository",
            land: null,
            sentence: CODING_SESSION_LANDING_NO_REPOSITORY,
          }
        : landRow(input.rule),
    landed: noRepository
      ? {
          tone: "neutral",
          word: "no repository",
          sentence: CODING_SESSION_LANDING_NO_REPOSITORY,
          checkedAt: null,
        }
      : landedRow(input.main, head?.sha ?? null, input.formatTime),
  };
}
