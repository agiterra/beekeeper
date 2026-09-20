import assert from "node:assert/strict";
import { test } from "node:test";

import {
  projectPulseDeclaredWork,
  shortSha,
} from "@/features/project-pulse/lib/pulseDeclaredWork";

const CHANNEL = "11111111-2222-3333-4444-555555555555";
const OTHER_CHANNEL = "99999999-2222-3333-4444-555555555555";
const SESSION = "aaaaaaaa-1111-2222-3333-444444444444";
const OTHER_SESSION = "bbbbbbbb-1111-2222-3333-444444444444";
const GENESIS = "5a".repeat(32);
const OTHER_GENESIS = "6b".repeat(32);
const PLANNER = "11".repeat(32);
const BUILDER = "22".repeat(32);
const LEAD = "33".repeat(32);
const FOUNDER = "44".repeat(32);
const NOW = 1_756_800_000;

function planEntry(overrides = {}) {
  return {
    eventId: "aa".repeat(32),
    pubkey: PLANNER,
    createdAt: NOW - 600,
    type: "plan",
    text: "Split the roles page into readable sections.",
    claimedAreas: ["desktop/src/features/projects"],
    branch: "work/roles-usability",
    sessionRef: null,
    supersedes: null,
    supersededBy: [],
    active: true,
    ...overrides,
  };
}

function digestOf(entries) {
  return {
    schema: "buzz-project-pulse-digest/v2",
    source: "client-composed",
    project: `30621:${FOUNDER}:pulse-demo`,
    asOf: NOW,
    complete: true,
    sessionsScope: "project channels",
    sessions: [],
    providerReachableSessions: [],
    openUnverifiedSessions: [],
    closedSessions: [],
    entries,
    errors: [],
  };
}

function settlement(overrides = {}) {
  return {
    settled: false,
    awaiting: { link: "disposition", owedByRole: "lead", owedByActor: null },
    governedReportEventId: null,
    dispositionEventId: null,
    acknowledgementEventId: null,
    ...overrides,
  };
}

function assignment(overrides = {}) {
  return {
    sourceEventId: "cc".repeat(32),
    createdAt: NOW - 300,
    assignerPubkey: LEAD,
    assigneeActor: BUILDER,
    assigneeRole: "builder",
    objective: "Page the declared-work read",
    brief: "Eight sessions per page, four pages, newest first.",
    branch: null,
    baseSha: null,
    fileOwnership: [],
    acceptanceSteps: [],
    supersedes: null,
    reports: [],
    dispositions: [],
    settlement: settlement(),
    status: "unresolved",
    ...overrides,
  };
}

function session(overrides = {}) {
  return {
    sessionKey: "s-1",
    sessionRef: SESSION,
    genesisRef: GENESIS,
    channelId: CHANNEL,
    name: "Declared work",
    lifecycle: "open",
    latestObservationAt: NOW - 120,
    founderPubkey: FOUNDER,
    terminal: null,
    unreadable: null,
    excludedCount: 0,
    assignments: [assignment()],
    ...overrides,
  };
}

/**
 * One loaded page: the response, and the session keys the page asked about.
 *
 * They differ exactly when a session's records could not be read — the gather
 * refuses to send it — so `askedKeys` is passed explicitly in that case.
 */
function page(sessions, errors = [], askedKeys = null) {
  return {
    response: {
      schema: "buzz-pulse-declared-work/v1",
      viewerPubkey: null,
      sessions,
      errors,
    },
    sessionKeys: askedKeys ?? sessions.map((session) => session.sessionKey),
  };
}

function project(overrides = {}) {
  return projectPulseDeclaredWork({
    digest: digestOf([]),
    pages: [],
    pageErrors: [],
    visibleSessionCount: 0,
    loadedPageCount: 0,
    nowSeconds: NOW,
    viewerPubkey: null,
    ...overrides,
  });
}

// ── Two participants, two sources, one list ─────────────────────────────────

test("a plan author and an assigned agent both appear, newest first", () => {
  const model = project({
    digest: digestOf([planEntry()]),
    pages: [
      page([
        session({ sessionKey: "s-1", assignments: [assignment()] }),
        session({
          sessionKey: "s-2",
          sessionRef: OTHER_SESSION,
          genesisRef: OTHER_GENESIS,
          channelId: OTHER_CHANNEL,
          name: "Second execution",
          assignments: [
            assignment({
              sourceEventId: "dd".repeat(32),
              createdAt: NOW - 900,
              assigneeActor: PLANNER,
              assigneeRole: "reviewer",
            }),
          ],
        }),
      ]),
    ],
    visibleSessionCount: 2,
    loadedPageCount: 1,
  });
  assert.deepEqual(
    model.current.map((row) => [row.kind, row.createdAt]),
    [
      ["assignment", NOW - 300],
      ["plan", NOW - 600],
      ["assignment", NOW - 900],
    ],
  );
  // The assigned actor is responsible; the assigning author stays inspectable
  // rather than being folded away into the row's identity.
  const assigned = model.current[0];
  assert.deepEqual(assigned.responsible, {
    pubkey: BUILDER,
    role: "builder",
  });
  assert.equal(assigned.assignedBy, LEAD);
  assert.equal(assigned.label, "Assigned");
  assert.equal(model.current[1].label, "Plan posted");
  assert.equal(model.current[1].entry.pubkey, PLANNER);
  assert.equal(model.scan.sentence, "Scanned all 2 visible sessions.");
  assert.deepEqual(model.limitations, []);
  assert.equal(model.noDeclaredWork, false);
});

test("a superseded plan is not a declaration and never appears twice", () => {
  const model = project({
    digest: digestOf([
      planEntry({ eventId: "ee".repeat(32), active: false }),
      planEntry(),
    ]),
    visibleSessionCount: 0,
    loadedPageCount: 0,
  });
  assert.deepEqual(
    model.current.map((row) => row.entry.eventId),
    ["aa".repeat(32)],
  );
});

test("only plans are regrouped; every other entry type stays where it was", () => {
  const model = project({
    digest: digestOf([
      planEntry({ eventId: "ff".repeat(32), type: "blocker" }),
      planEntry(),
    ]),
  });
  assert.equal(model.current.length, 1);
  assert.equal(model.current[0].entry.type, "plan");
});

// ── Closing settles nothing ─────────────────────────────────────────────────

test("an unresolved assignment in a closed session stays current, with evidence", () => {
  const model = project({
    pages: [page([session({ lifecycle: "closed" })])],
    visibleSessionCount: 1,
    loadedPageCount: 1,
  });
  assert.equal(model.current.length, 1);
  assert.deepEqual(model.settled, []);
  const closed = model.current[0].evidence.find(
    (line) => line.label === "Session closed",
  );
  assert.notEqual(closed, undefined);
  assert.match(closed.detail, /Closing settles nothing/);
  assert.equal(closed.eventId, null);
});

test("settlement comes from the fold's own row, and moves the row to settled", () => {
  const model = project({
    pages: [
      page([
        session({
          assignments: [
            assignment({
              status: "settled",
              settlement: settlement({
                settled: true,
                awaiting: null,
                governedReportEventId: "1a".repeat(32),
                dispositionEventId: "2b".repeat(32),
                acknowledgementEventId: "3c".repeat(32),
              }),
            }),
          ],
        }),
      ]),
    ],
    visibleSessionCount: 1,
    loadedPageCount: 1,
  });
  assert.deepEqual(model.current, []);
  assert.equal(model.settled.length, 1);
  const settledLine = model.settled[0].evidence.find(
    (line) => line.label === "Settled",
  );
  assert.equal(settledLine.eventId, "2b".repeat(32));
  // A settled assignment's session closure is not restated as settlement.
  assert.equal(
    model.settled[0].evidence.some((line) => line.label === "Session closed"),
    false,
  );
});

// ── Evidence is evidence ────────────────────────────────────────────────────

test("a report is evidence of a report, with its age, head and unseated author", () => {
  const model = project({
    pages: [
      page([
        session({
          terminal: {
            eventId: "9f".repeat(32),
            type: "mission.completed",
            at: NOW - 60,
          },
          assignments: [
            assignment({
              status: "reported",
              reports: [
                {
                  eventId: "4d".repeat(32),
                  authorPubkey: BUILDER,
                  createdAt: NOW - 7_200,
                  summary: "Paged the read and pinned the sentence.",
                  branch: "work/declared",
                  baseSha: "abc123",
                  headSha: "0123456789abcdef",
                  files: [],
                  testCount: 3,
                  deviations: [],
                  residuals: [],
                  authorUnseated: true,
                },
              ],
              dispositions: [
                {
                  eventId: "5e".repeat(32),
                  authorPubkey: LEAD,
                  createdAt: NOW - 3_600,
                  decision: "changes-requested",
                  reportRef: "4d".repeat(32),
                },
              ],
            }),
          ],
        }),
      ]),
    ],
    visibleSessionCount: 1,
    loadedPageCount: 1,
  });
  const [row] = model.current;
  assert.deepEqual(
    row.evidence.map((line) => line.label),
    ["Report submitted", "Disposition", "Mission completed"],
  );
  const report = row.evidence[0];
  assert.match(
    report.detail,
    /^2h ago · Paged the read and pinned the sentence\./,
  );
  assert.match(report.detail, /head 01234567/);
  assert.match(report.detail, /author holds no seat for builder/);
  assert.equal(report.eventId, "4d".repeat(32));
  // The app's one compact pubkey form (`truncatePubkey`), never a second
  // hand-rolled shape.
  assert.match(
    row.evidence[1].detail,
    /^Changes requested by 33333333…3333 · 1h ago$/,
  );
  // A report never reads as "done": no evidence label says so, and the
  // assignment stays in `current`.
  assert.equal(model.settled.length, 0);
});

test("a missing branch, base or declared path is carried as missing", () => {
  const model = project({
    pages: [page([session()])],
    visibleSessionCount: 1,
    loadedPageCount: 1,
  });
  const { assignment: carried } = model.current[0];
  assert.equal(carried.branch, null);
  assert.equal(carried.baseSha, null);
  assert.deepEqual(carried.fileOwnership, []);
});

test("declared paths are carried verbatim and compared with nothing", () => {
  const paths = ["desktop/src/features/project-pulse/lib/", "crates/buzz-core"];
  const model = project({
    pages: [
      page([
        session({ assignments: [assignment({ fileOwnership: paths })] }),
        session({
          sessionKey: "s-2",
          sessionRef: OTHER_SESSION,
          assignments: [
            assignment({
              sourceEventId: "6f".repeat(32),
              fileOwnership: ["crates/buzz-core"],
            }),
          ],
        }),
      ]),
    ],
    visibleSessionCount: 2,
    loadedPageCount: 1,
  });
  assert.deepEqual(model.current[0].assignment.fileOwnership, paths);
  const prose = JSON.stringify(model);
  for (const word of ["overlap", "conflict", "Overlap", "Conflict"]) {
    assert.equal(
      prose.includes(word),
      false,
      `the projection must not compare declared scopes: found "${word}"`,
    );
  }
});

// ── Reads that did not finish ───────────────────────────────────────────────

test("an unreadable session is a limitation, never 'no declared work'", () => {
  const model = project({
    pages: [
      page(
        [
          session({
            assignments: [],
            unreadable: "this session's team records did not fold",
          }),
        ],
        [
          {
            scope: "declared:s-1",
            message: "this session's team records did not fold",
          },
        ],
      ),
    ],
    visibleSessionCount: 1,
    loadedPageCount: 1,
  });
  assert.deepEqual(model.current, []);
  assert.equal(model.noDeclaredWork, false);
  assert.equal(model.scan.unreadableSessions, 1);
  // Once, not twice: the command mirrors the session's own sentence into
  // `errors`, and printing both says the same failure two ways.
  assert.deepEqual(model.limitations, [
    "The records of Declared work could not be read: this session's team records did not fold",
  ]);
});

test("a page that failed keeps the pages that did not", () => {
  const model = project({
    pages: [page([session()])],
    pageErrors: [{ pageIndex: 1, message: "relay unreachable" }],
    visibleSessionCount: 12,
    loadedPageCount: 1,
  });
  assert.equal(model.current.length, 1);
  assert.deepEqual(model.limitations, [
    "Page 2 of the session scan could not be read: relay unreachable",
  ]);
  assert.equal(model.noDeclaredWork, false);
  assert.equal(
    model.scan.sentence,
    "Scanned 1 of 12 visible sessions, newest first; 11 older sessions not read yet.",
  );
});

test("excluded records are disclosed as a count, never as their content", () => {
  const model = project({
    pages: [page([session({ excludedCount: 2 })])],
    visibleSessionCount: 1,
    loadedPageCount: 1,
  });
  assert.deepEqual(model.limitations, [
    "2 records in Declared work were excluded by the canonical fold and are not shown.",
  ]);
});

test("a successful empty read inside its scope may say 'no declared work'", () => {
  const model = project({ visibleSessionCount: 0, loadedPageCount: 0 });
  assert.equal(model.noDeclaredWork, true);
  assert.equal(
    model.scan.sentence,
    "No sessions are visible in this project's channels to scan.",
  );
  assert.equal(model.scan.morePages, false);
  assert.equal(model.scan.capped, false);
});

// ── The scan's own bounds ───────────────────────────────────────────────────

test("an unread page is offered, and the sentence counts what is left", () => {
  const model = project({
    pages: [page([])],
    visibleSessionCount: 13,
    loadedPageCount: 1,
  });
  assert.equal(model.scan.morePages, true);
  assert.equal(model.scan.capped, false);
  assert.equal(
    model.scan.sentence,
    "Scanned 0 of 13 visible sessions, newest first; 13 older sessions not read yet.",
  );
});

test("the scan says where it stops rather than advertising completeness", () => {
  const sessions = Array.from({ length: 32 }, (_value, index) =>
    session({
      sessionKey: `s-${index}`,
      sessionRef: `${String(index % 10).repeat(8)}-1111-2222-3333-444444444444`,
      assignments: [],
    }),
  );
  const model = project({
    pages: [
      page(sessions.slice(0, 8)),
      page(sessions.slice(8, 16)),
      page(sessions.slice(16, 24)),
      page(sessions.slice(24, 32)),
    ],
    visibleSessionCount: 40,
    loadedPageCount: 4,
  });
  assert.equal(model.scan.scannedSessions, 32);
  assert.equal(model.scan.capped, true);
  assert.equal(model.scan.morePages, false);
  assert.equal(
    model.scan.sentence,
    "Scanned 32 of 40 visible sessions, newest first; the read stops at 32 sessions.",
  );
  assert.deepEqual(model.limitations, [
    "This scan reads at most 32 sessions; older sessions were not read.",
  ]);
  assert.equal(model.noDeclaredWork, false);
});

test("one older session reads as one, not as '1 older sessions'", () => {
  const scanned = Array.from({ length: 8 }, (_value, index) =>
    session({ sessionKey: `s-${index}`, assignments: [] }),
  );
  const model = project({
    pages: [page(scanned)],
    visibleSessionCount: 9,
    loadedPageCount: 1,
  });
  assert.equal(
    model.scan.sentence,
    "Scanned 8 of 9 visible sessions, newest first; 1 older session not read yet.",
  );
});

// ── Dedupe ──────────────────────────────────────────────────────────────────

test("a session that moved across a page boundary is one row, not two", () => {
  const first = session();
  const model = project({
    pages: [page([first]), page([first])],
    visibleSessionCount: 9,
    loadedPageCount: 2,
  });
  assert.equal(model.current.length, 1);
  assert.equal(
    model.current[0].dedupeKey,
    `${CHANNEL}:${SESSION}:${"cc".repeat(32)}`,
  );
});

test("the dedupe key separates the same source event in different channels", () => {
  const model = project({
    pages: [
      page([
        session(),
        session({
          sessionKey: "s-2",
          channelId: OTHER_CHANNEL,
          sessionRef: OTHER_SESSION,
        }),
      ]),
    ],
    visibleSessionCount: 2,
    loadedPageCount: 1,
  });
  assert.equal(model.current.length, 2);
  assert.notEqual(model.current[0].dedupeKey, model.current[1].dedupeKey);
});

test("a re-read that reordered pages keeps the newer record of one key", () => {
  const stale = session({
    assignments: [assignment({ createdAt: NOW - 900 })],
  });
  const fresh = session({
    assignments: [assignment({ createdAt: NOW - 100 })],
  });
  const model = project({
    pages: [page([stale]), page([fresh])],
    visibleSessionCount: 9,
    loadedPageCount: 2,
  });
  assert.equal(model.current.length, 1);
  assert.equal(model.current[0].createdAt, NOW - 100);
});

// ── Nothing outside the pages ───────────────────────────────────────────────

test("only the loaded pages and the digest's plans can produce a row", () => {
  const model = project({
    digest: digestOf([planEntry()]),
    pages: [],
    visibleSessionCount: 4,
    loadedPageCount: 0,
  });
  // A session that exists, was never read, and belongs to some other project's
  // channel cannot appear: there is no source for it in this input at all.
  assert.deepEqual(
    model.current.map((row) => row.kind),
    ["plan"],
  );
  assert.equal(model.scan.scannedSessions, 0);
});

test("a row carries its session's genesis id verbatim, never a derived one", () => {
  const model = project({
    pages: [
      page([
        session(),
        session({
          sessionKey: "s-2",
          sessionRef: OTHER_SESSION,
          genesisRef: OTHER_GENESIS,
          channelId: OTHER_CHANNEL,
        }),
      ]),
    ],
    visibleSessionCount: 2,
    loadedPageCount: 1,
  });
  // The session ref is author-chosen; the genesis record is what names the
  // channel and the founder the 44244 set was proven under. Both travel, so
  // the disclosure block can show the record rather than a pointer to it.
  assert.deepEqual(
    model.current.map((row) => [
      row.session.sessionRef,
      row.session.genesisRef,
    ]),
    [
      [SESSION, GENESIS],
      [OTHER_SESSION, OTHER_GENESIS],
    ],
  );
});

// ── A complete read, and what "complete" is allowed to mean ─────────────────

test("a complete read whose only work is settled is complete, not empty", () => {
  const model = project({
    pages: [
      page([
        session({
          assignments: [
            assignment({
              status: "settled",
              settlement: settlement({
                settled: true,
                awaiting: null,
                governedReportEventId: "1a".repeat(32),
                dispositionEventId: "2b".repeat(32),
                acknowledgementEventId: "3c".repeat(32),
              }),
            }),
          ],
        }),
      ]),
    ],
    visibleSessionCount: 1,
    loadedPageCount: 1,
  });
  // The read covered its scope; it simply found settled work. Reporting it as
  // incomplete would print "the read is incomplete" over a section showing
  // everything there is.
  assert.equal(model.readIsComplete, true);
  assert.equal(model.noDeclaredWork, false);
  assert.equal(model.settled.length, 1);
  assert.deepEqual(model.limitations, []);
  assert.equal(model.scan.sentence, "Scanned the 1 visible session.");
});

test("an incomplete read may not say 'no declared work' about what it skipped", () => {
  const model = project({
    pages: [page([])],
    visibleSessionCount: 13,
    loadedPageCount: 1,
  });
  // Five pages' worth of sessions are unread. An empty list here is "we have
  // not looked", and it may not wear the same words as "there is nothing".
  assert.equal(model.readIsComplete, false);
  assert.equal(model.noDeclaredWork, false);
});

// ── A session that was reached and could not be read ────────────────────────

test("a session the gather dropped is 'could not be read', not 'not read yet'", () => {
  const model = project({
    pages: [
      page(
        [],
        [
          {
            scope: "declared:s-1",
            message: "the team record read was truncated at 500 events",
          },
        ],
        ["s-1"],
      ),
    ],
    visibleSessionCount: 1,
    loadedPageCount: 1,
  });
  assert.equal(model.scan.droppedSessions, 1);
  assert.equal(model.scan.scannedSessions, 0);
  // Never "1 older session not read yet": there is no control behind that
  // sentence for this session, and pressing "Show older sessions" would not
  // surface it.
  assert.equal(
    model.scan.sentence,
    "Scanned 0 of 1 visible session, newest first; 1 could not be read.",
  );
  assert.equal(model.scan.morePages, false);
  assert.equal(model.readIsComplete, false);
  assert.equal(model.noDeclaredWork, false);
  assert.deepEqual(model.limitations, [
    "declared:s-1: the team record read was truncated at 500 events",
  ]);
});

test("a dropped session does not shrink the count of sessions still unread", () => {
  const scanned = Array.from({ length: 7 }, (_value, index) =>
    session({ sessionKey: `s-${index}`, assignments: [] }),
  );
  const model = project({
    pages: [page(scanned, [], [...scanned.map((s) => s.sessionKey), "s-7"])],
    visibleSessionCount: 12,
    loadedPageCount: 1,
  });
  assert.equal(model.scan.scannedSessions, 7);
  assert.equal(model.scan.droppedSessions, 1);
  assert.equal(
    model.scan.sentence,
    "Scanned 7 of 12 visible sessions, newest first; 4 older sessions not read yet; 1 could not be read.",
  );
});

// ── An unreadable session is spoken, not merely counted ─────────────────────

test("an unreadable session is named in the scan sentence, not only counted", () => {
  const model = project({
    pages: [
      page([
        session({ assignments: [] }),
        session({
          sessionKey: "s-2",
          sessionRef: OTHER_SESSION,
          genesisRef: OTHER_GENESIS,
          assignments: [],
          unreadable: "this session's team records did not fold",
        }),
      ]),
    ],
    visibleSessionCount: 2,
    loadedPageCount: 1,
  });
  assert.equal(model.scan.unreadableSessions, 1);
  assert.equal(
    model.scan.sentence,
    "Scanned 2 of 2 visible sessions, newest first; 1 session's records could not be read.",
  );
  assert.equal(model.readIsComplete, false);
});

test("two unreadable sessions read as two, with the plural possessive", () => {
  const model = project({
    pages: [
      page([
        session({ assignments: [], unreadable: "did not fold" }),
        session({
          sessionKey: "s-2",
          sessionRef: OTHER_SESSION,
          genesisRef: OTHER_GENESIS,
          assignments: [],
          unreadable: "did not fold",
        }),
      ]),
    ],
    visibleSessionCount: 2,
    loadedPageCount: 1,
  });
  assert.match(model.scan.sentence, /2 sessions' records could not be read\.$/);
});

// ── One convention for a short sha ──────────────────────────────────────────

test("a short sha is the first eight characters, for base and head alike", () => {
  assert.equal(shortSha("0123456789abcdef0123456789abcdef"), "01234567");
  assert.equal(shortSha("abc123"), "abc123");
});
