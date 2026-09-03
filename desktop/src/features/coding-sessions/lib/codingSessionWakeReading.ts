/**
 * One line for an identifier-only team wake — the same line in both lenses.
 *
 * A team wake is a turn whose whole text is a pointer: either
 * `{"operationId","type"}` or the five-field `buzz-team-wake/v1` object, both
 * minted by {@link codingSessionTeamWakeText}. The wire is deliberately
 * identifier-only, so a seat reads the operation from the fold rather than
 * from prose it would have to trust. A **person** was never given that
 * courtesy: live run 2 (2026-09-02, 10:46) rendered the founder's own
 * `decision.answer` wake into the Conversation lens as
 * `{"operationId":"4847ff06…","type":"decision.answer"}` in a "You" bubble —
 * correct attribution, unreadable rendering (finding 17).
 *
 * The rules this module holds to:
 *
 * - **Only the two minted shapes are read.** Anything else is prose and comes
 *   back `null`, so an ordinary turn renders exactly as it did before.
 * - **The pointer's `type` chooses the sentence; the fold supplies its
 *   subject.** Nothing is ever parsed out of the words of a turn (batch
 *   invariant I5) and nothing is guessed: an operation the fold does not hold
 *   yields the unresolved line, never an invented subject.
 * - **The raw JSON stays available** in Trace and the Inspector, which render
 *   the signed record itself. It simply stops being what a reader is handed
 *   in a chat bubble.
 *
 * The sentence table is frozen in `review-2026-09-01/00-BATCH.md` §1f.
 */

/** The two pointer shapes `codingSessionTeamWakeText` can mint. */
export type CodingSessionWakePointer =
  | { kind: "operation"; operationId: string; type: string }
  | {
      kind: "terminal";
      type: string;
      terminalEventId: string;
      seatRole: string;
    };

/**
 * One fold-resolved operation, reduced to what a reading needs.
 *
 * Every field is a signed value already projected by the Rust fold's own
 * output; this module never derives one. `subjectEventId` is the event the
 * sentence names — an assignment for a report, a report for a verdict, a
 * request for an answer — which the stream projection has already resolved
 * into the row's `parentEventId`.
 */
export type CodingSessionWakeOperationFact = {
  /** The operation's own signed event id. */
  eventId: string;
  /**
   * The key that signed the operation.
   *
   * REVIEW-L2 F2: without this the reading composed `{Who}` from the **turn's**
   * signer and its subject from the **operation's** record and never asked
   * whether they were the same identity — so a member who typed the founder's
   * own pointer into the composer was rendered performing the founder's signed
   * act ("Mallory answered decision 2099cdb3: C…"). The exact-field parse stops
   * the accident; only this stops the act.
   */
  authorPubkey: string;
  /**
   * The stream type of the record, as the fold projected it.
   *
   * REVIEW-L2 F3: a pointer's `type` is a claim in a turn's own text. Joining
   * on the id alone let `{"operationId": <a report>, "type": "verdict"}`
   * produce `You ruled on report <the assignment's 8hex>` — a confident
   * sentence naming the wrong event.
   */
  type: string;
  /** The event this operation's sentence names, when it names one. */
  subjectEventId: string | null;
  /** An assignment's signed `assigneeRole`. */
  role: string | null;
  /** The signed words this sentence quotes: an objective, question or choice. */
  subject: string | null;
  /**
   * A `decision.answer`'s signed `condition`, when it carried one.
   *
   * Text, never a predicate: this surface quotes it and nothing more. It is
   * appended to the answer's reading because the reason it exists — so the
   * same question is not asked once per commit (finding 21) — only works if
   * the next reader can see it where the answer is read.
   *
   * Absent (`null`) for every other operation type, and for an answer that
   * named no class.
   */
  condition?: string | null;
};

/** Fold-resolved operations, keyed by their own event id. */
export type CodingSessionWakeOperationIndex = ReadonlyMap<
  string,
  CodingSessionWakeOperationFact
>;

/**
 * How much signed prose one reading may quote before it is elided.
 *
 * A wake line is a *line*. An objective or a chosen option can run to several
 * hundred characters on the wire (live run 2's own answer option is 52, its
 * question 283), and a bubble that reflows to eight lines is the same failure
 * as the raw JSON in a different costume. The full text is one row away in the
 * Mission stream, which renders the signed record itself.
 */
export const MAX_CODING_SESSION_WAKE_READING_SUBJECT_CHARS = 120;

const HEX64 = /^[0-9a-f]{64}$/;
const TERMINAL_WAKE_SCHEMA = "buzz-team-wake/v1";

/**
 * Read a turn's text as a wake pointer, or `null` when it is prose.
 *
 * Exact-field on purpose, in both directions: a JSON object that merely
 * *contains* an `operationId` is not a wake, and treating it as one would let
 * any member turn a person's own words into a system sentence by typing JSON.
 */
export function parseCodingSessionWakePointer(
  text: string,
): CodingSessionWakePointer | null {
  const trimmed = text.trim();
  if (!trimmed.startsWith("{") || !trimmed.endsWith("}")) return null;
  let value: unknown;
  try {
    value = JSON.parse(trimmed);
  } catch {
    return null;
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record).sort().join(",");
  if (keys === "operationId,type") {
    return typeof record.operationId === "string" &&
      HEX64.test(record.operationId) &&
      typeof record.type === "string" &&
      record.type.length > 0
      ? {
          kind: "operation",
          operationId: record.operationId,
          type: record.type,
        }
      : null;
  }
  if (
    keys === "causedByCommandId,schema,seatRole,terminalEventId,type" &&
    record.schema === TERMINAL_WAKE_SCHEMA &&
    typeof record.type === "string" &&
    record.type.length > 0 &&
    typeof record.terminalEventId === "string" &&
    HEX64.test(record.terminalEventId) &&
    typeof record.seatRole === "string" &&
    record.seatRole.length > 0
  ) {
    return {
      kind: "terminal",
      type: record.type,
      terminalEventId: record.terminalEventId,
      seatRole: record.seatRole,
    };
  }
  return null;
}

/** The first eight hex characters of an event id — the id form §1f freezes. */
export function codingSessionWakeShortId(eventId: string): string {
  return eventId.slice(0, 8);
}

/**
 * The one line a wake pointer reads as, for a viewer who calls its author
 * `who`.
 *
 * `who` is resolved by the caller from exactly the attribution the surface
 * already renders (`You` for the viewer, else a display name, else the first
 * eight hex of the author key) — this module never resolves an identity,
 * because two surfaces resolving it twice is how one of them ends up naming
 * somebody else.
 */
export function codingSessionWakeReading(input: {
  pointer: CodingSessionWakePointer;
  who: string;
  /**
   * The key that signed the **turn** this pointer arrived in, or null when the
   * transcript carried no operator stamp. Compared against the operation's own
   * author (F2); `null` never matches.
   */
  signerPubkey: string | null;
  operations: CodingSessionWakeOperationIndex;
}): string {
  const { pointer, who } = input;
  if (pointer.kind === "terminal") {
    return `${who} woke ${pointer.seatRole} for ${pointer.type}`;
  }
  const fact = input.operations.get(pointer.operationId) ?? null;
  const short = codingSessionWakeShortId(pointer.operationId);
  const unresolved = (reason: string) =>
    `${who} sent a wake for operation ${short} (${pointer.type}) — ${reason}`;
  const notInRecords = unresolved("not in this session's records yet");
  if (fact === null) {
    // §1f amendment (2026-09-02): a lens that holds **no** fold at all has not
    // looked this operation up and found nothing — it has not looked. Saying
    // "not in this session's records" there is a claim about the wire made
    // from the absence of a local index, which is critique A1's exact shape.
    return input.operations.size === 0
      ? `${who} sent a wake for operation ${short} — this lens holds no session records; open Mission to read it.`
      : notInRecords;
  }
  // F2. Identity, not spelling: both sides are folded before comparing, and an
  // unknown signer is never a match.
  const signer = input.signerPubkey?.trim().toLowerCase() ?? null;
  if (signer === null || signer !== fact.authorPubkey.trim().toLowerCase()) {
    return unresolved(`signed by ${who}, not the operation's author`);
  }
  // F3. The record's own word decides what it is; the pointer only names it.
  if (!wakeTypeMatchesRecord(pointer.type, fact.type)) {
    return unresolved(`the record is a ${fact.type}, not a ${pointer.type}`);
  }
  const subject = clampSubject(fact.subject);
  const named =
    fact.subjectEventId === null
      ? null
      : codingSessionWakeShortId(fact.subjectEventId);
  switch (pointer.type) {
    case "assignment":
      return fact.role === null
        ? notInRecords
        : subject === null
          ? `${who} assigned ${fact.role}`
          : `${who} assigned ${fact.role}: ${subject}`;
    case "report":
      return named === null
        ? notInRecords
        : `${who} reported on assignment ${named}`;
    case "verdict":
      return named === null ? notInRecords : `${who} ruled on report ${named}`;
    case "acknowledgement":
      return named === null
        ? notInRecords
        : `${who} acknowledged verdict ${named}`;
    case "mission.completed":
      return `${who} closed the mission as completed`;
    case "mission.blocked":
      return `${who} marked the mission blocked`;
    case "note":
      return `${who} left a note`;
    case "decision.request": {
      const asked = codingSessionWakeShortId(fact.eventId);
      return subject === null
        ? `${who} asked decision ${asked}`
        : `${who} asked decision ${asked}: ${subject}`;
    }
    case "decision.answer": {
      // §1f: the id shown is the **request's**, never the answer's own. The
      // request is what a reader is holding in their head; the answer's id is
      // a fact for the Inspector.
      if (named === null) return notInRecords;
      const answered =
        subject === null
          ? `${who} answered decision ${named}`
          : `${who} answered decision ${named}: ${subject}`;
      const condition = clampSubject(fact.condition ?? null);
      return condition === null
        ? answered
        : `${answered} — condition: ${condition}`;
    }
    default:
      return notInRecords;
  }
}

/**
 * Whether a pointer's wire `type` names the kind of record the fold holds.
 *
 * `verdict` is the one wire type the stream splits, by the record's own signed
 * `subtype` — so a `verdict` pointer legitimately lands on a `refutation` or a
 * `disposition` row. Every other type is compared verbatim, and an unknown
 * pairing is a mismatch rather than a benefit of the doubt.
 */
function wakeTypeMatchesRecord(
  pointerType: string,
  recordType: string,
): boolean {
  if (pointerType === recordType) return true;
  return (
    pointerType === "verdict" &&
    (recordType === "refutation" || recordType === "disposition")
  );
}

/**
 * Read a turn's text as a wake line, or `null` when it is not a wake.
 *
 * The one entry point a renderer needs: prose returns `null` and is rendered
 * exactly as before, so the Conversation lens moves for wake bubbles and for
 * nothing else.
 */
export function codingSessionWakeReadingForText(input: {
  text: string;
  who: string;
  /** The turn's own signer; see {@link codingSessionWakeReading}. */
  signerPubkey: string | null;
  operations: CodingSessionWakeOperationIndex;
}): string | null {
  const pointer = parseCodingSessionWakePointer(input.text);
  return pointer === null
    ? null
    : codingSessionWakeReading({
        operations: input.operations,
        pointer,
        signerPubkey: input.signerPubkey,
        who: input.who,
      });
}

/**
 * Build the operation index from the fold rows this surface already holds.
 *
 * `transactions` are the fold-included 44244 rows the Mission stream renders;
 * `assignments` are the same fold's assignment bodies, which carry the signed
 * `assigneeRole` and `objective` a `report`'s and an `assignment`'s sentences
 * need. Nothing else is consulted, so a reading can never say more than the
 * fold does.
 */
export function buildCodingSessionWakeOperationIndex(input: {
  transactions: readonly {
    sourceEventId: string;
    type: string;
    authorPubkey: string;
    parentEventId: string | null;
    summary: string;
    /**
     * A `decision.answer` row's signed `condition`, when the projection
     * carries one. Optional so a caller that has not been widened yet is
     * unchanged: an absent field reads as "no condition", exactly as an
     * answer that named none does.
     */
    condition?: string | null;
  }[];
  assignments: readonly {
    sourceEventId: string;
    assigneeRole: string;
    objective: string;
  }[];
}): CodingSessionWakeOperationIndex {
  const assignmentById = new Map(
    input.assignments.map(
      (assignment) => [assignment.sourceEventId, assignment] as const,
    ),
  );
  const index = new Map<string, CodingSessionWakeOperationFact>();
  for (const row of input.transactions) {
    const assignment = assignmentById.get(row.sourceEventId) ?? null;
    index.set(row.sourceEventId, {
      eventId: row.sourceEventId,
      authorPubkey: row.authorPubkey,
      type: row.type,
      subjectEventId: row.parentEventId,
      role: assignment?.assigneeRole ?? null,
      subject: assignment
        ? nonEmpty(assignment.objective)
        : nonEmpty(row.summary),
      condition: row.condition ?? null,
    });
  }
  // An assignment with no stream row is deliberately **not** added. It used to
  // be, as belt-and-braces for a row the projection had left without a
  // summary — but F2 makes the record's own signer load-bearing and an
  // assignment body carries no author here, so an entry built from it could
  // only be authorless. The fold includes a row for every included assignment,
  // so nothing real is lost; a guess about who signed one would not be.
  return index;
}

function nonEmpty(value: string): string | null {
  const trimmed = value.trim();
  return trimmed.length === 0 ? null : trimmed;
}

function clampSubject(value: string | null): string | null {
  if (value === null) return null;
  const collapsed = value.trim().replace(/\s+/g, " ");
  if (collapsed.length === 0) return null;
  return collapsed.length <= MAX_CODING_SESSION_WAKE_READING_SUBJECT_CHARS
    ? collapsed
    : `${collapsed.slice(0, MAX_CODING_SESSION_WAKE_READING_SUBJECT_CHARS - 1)}…`;
}

// -- L5.4: Conversation resolves from cache, and never fetches --------------

/**
 * The newest Mission operation index, keyed by the exact scope it was folded
 * for.
 *
 * **A cache, not a subscription.** REPORT-L2 6c left Conversation's wake line
 * unresolved because that lens subscribes to no fold, and REVIEW-L2 advised
 * against giving it one - a per-session live relay subscription in the
 * one-seat view is a real cost, and a per-pointer resolve is worse: the answer
 * is a property of the *whole* 44244 set the fold takes, so it is either N
 * folds or a cache that is the subscription renamed.
 *
 * What this is instead costs nothing. When Mission has already folded this
 * session, Conversation reads that fold's index and prints the resolved line.
 * When it has not, the amended line `this lens holds no session records; open
 * Mission to read it.` stands. Both sentences are true about what the lens
 * holds, which is the test.
 *
 * **One entry.** Not a map that grows: the reader is looking at one session,
 * and a second entry would only ever be a session nobody is looking at. The
 * key carries the channel UUID, the umbrella, the genesis and the founder, so
 * a hit is the same session by construction and a community switch cannot
 * produce one.
 */
let cachedWakeOperations: {
  key: string;
  operations: CodingSessionWakeOperationIndex;
} | null = null;

const EMPTY_WAKE_OPERATIONS: CodingSessionWakeOperationIndex = new Map();

/** The exact scope a cached index belongs to. */
export type CodingSessionWakeOperationScope = {
  channelRef: string;
  sessionRef: string | null;
  genesisRef: string | null;
  founderPubkey: string | null;
};

/**
 * The cache key, or `null` for a scope that is not fully known.
 *
 * A partial scope never reads and never writes: an index stored under half a
 * key could be handed to a different session that happens to share the half.
 */
export function codingSessionWakeOperationScopeKey(
  scope: CodingSessionWakeOperationScope,
): string | null {
  if (
    scope.sessionRef === null ||
    scope.genesisRef === null ||
    scope.founderPubkey === null
  ) {
    return null;
  }
  return [
    scope.channelRef,
    scope.sessionRef,
    scope.genesisRef,
    scope.founderPubkey,
  ].join(" ");
}

/** Remember the index Mission just folded, for the lens that folds nothing. */
export function rememberCodingSessionWakeOperations(
  scope: CodingSessionWakeOperationScope,
  operations: CodingSessionWakeOperationIndex,
): void {
  const key = codingSessionWakeOperationScopeKey(scope);
  if (key === null || operations.size === 0) return;
  cachedWakeOperations = { key, operations };
}

/**
 * The cached index for this exact scope, or an empty one.
 *
 * Empty is the honest answer for a cold cache, and it is what makes the
 * no-fold line fire. Nothing here fetches, invokes or subscribes.
 */
export function readCachedCodingSessionWakeOperations(
  scope: CodingSessionWakeOperationScope,
): CodingSessionWakeOperationIndex {
  const key = codingSessionWakeOperationScopeKey(scope);
  if (key === null || cachedWakeOperations?.key !== key) {
    return EMPTY_WAKE_OPERATIONS;
  }
  return cachedWakeOperations.operations;
}

/**
 * Forget the cached index.
 *
 * Community-scoped module state has to be resettable or it leaks across a
 * community switch (AGENTS.md section "Community Switching"). The
 * single-entry, fully-keyed design means a stale entry can never be *read* by
 * another community; this exists so the memory is released and so
 * `resetCommunityState()` has something to call.
 */
export function resetCodingSessionWakeOperations(): void {
  cachedWakeOperations = null;
}
