import assert from "node:assert/strict";
import test from "node:test";
import * as React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { RolePackSnapshots } from "./RolePackSnapshots.tsx";

const coordinate = {
  repo: "30617:owner:packs",
  sha: "a".repeat(40),
  role: "builder",
  path: "personas/roles/builder",
};
const MEMBER_PUBKEY = "f".repeat(64);

function reportedRow(overrides = {}) {
  return {
    channelId: "project-transport",
    generationId: "generation-4",
    label: "Build the snapshot",
    claimedByPubkey: MEMBER_PUBKEY,
    reportedAt: 1_700_000_001_000,
    coordinate,
    reason: null,
    relation: "current",
    behind: null,
    ahead: null,
    note: null,
    status: "running",
    ageSeconds: 300,
    adoption: "none",
    provenance: "proof-unavailable",
    provenanceReason: "no accepted lifecycle proof for this generation",
    founderPubkey: null,
    role: "builder",
    ...overrides,
  };
}

test("states the checkout commit and when git answered, and marks every metadata claim unverified", () => {
  const html = renderToStaticMarkup(
    React.createElement(RolePackSnapshots, {
      snapshots: {
        resolvedAt: 1_700_000_000_000,
        resolved: [{ role: "builder", coordinate, reason: null }],
        reported: [reportedRow({ relation: "current" })],
        comparison: {
          currentSha: coordinate.sha,
          comparedAt: 1_700_000_000_900,
          reason: null,
        },
        provenanceNotes: [],
      },
      resolvedError: "native resolver timed out",
      resolvedIsStale: true,
      revisionsError: null,
      reports: {
        isLoading: false,
        error: "one visible channel did not answer",
        authorityError: null,
      },
    }),
  );

  assert.match(html, /Checked at/);
  assert.match(html, /On aaaaaaaa · git answered at/);
  assert.match(
    html,
    /last refresh failed; rows below are from the earlier check/,
  );
  assert.match(
    html,
    /Metadata claims may be incomplete: one visible channel did not answer/,
  );
  assert.match(
    html,
    /Unverified · proof unavailable · no accepted lifecycle proof for this generation/,
  );
  assert.match(html, /from <span[^>]+>ffffffff…ffff<\/span>/);
  assert.match(html, /Same revision as this machine/);
  assert.match(html, /reported 5m ago/);
  assert.match(html, /Report history \(1\)/);
  assert.match(
    html,
    /Beekeeper checks who sent each report\. Confirming its source does not prove which role instructions were used\./,
  );
  // Never word `commissioned` as verified execution or adoption.
  assert.doesNotMatch(html, /verified execution|verified adoption/);
  // A current row never carries the adoption sentence.
  assert.doesNotMatch(html, /Keeps this revision until its next launch/);
  assert.match(html, /Technical details/);
  assert.doesNotMatch(html, /Reported by sessions/);
  assert.doesNotMatch(html, /universally adopted|every machine is current/);
});

test("discloses a failed revision comparison instead of guessing a relation", () => {
  const html = renderToStaticMarkup(
    React.createElement(RolePackSnapshots, {
      snapshots: {
        resolvedAt: 1_700_000_000_000,
        resolved: [{ role: "builder", coordinate, reason: null }],
        reported: [
          reportedRow({
            relation: "unknown-here",
            status: "running",
            adoption: "keeps-until-next-generation",
          }),
        ],
        comparison: null,
        provenanceNotes: [],
      },
      resolvedError: null,
      resolvedIsStale: false,
      revisionsError: "git exited 1",
      reports: { isLoading: false, error: null, authorityError: null },
    }),
  );

  assert.match(html, /Revision comparison unavailable: git exited 1/);
  assert.match(html, /Revision unknown to this machine/);
  assert.match(html, /Keeps this revision until its next launch or resume/);
  // Never render a row as current while the comparison failed.
  assert.doesNotMatch(html, /Same revision as this machine/);
});

test("shows the earlier/later distance and the terminal status without an adoption sentence", () => {
  const html = renderToStaticMarkup(
    React.createElement(RolePackSnapshots, {
      snapshots: {
        resolvedAt: null,
        resolved: [],
        reported: [
          reportedRow({
            generationId: "generation-earlier",
            relation: "earlier",
            behind: 4,
            status: "completed",
            adoption: "none",
          }),
        ],
        comparison: null,
        provenanceNotes: [],
      },
      resolvedError: null,
      resolvedIsStale: false,
      revisionsError: null,
      reports: { isLoading: false, error: null, authorityError: null },
    }),
  );

  assert.match(html, /Earlier revision · 4 behind this machine/);
  assert.match(html, />completed</);
  assert.doesNotMatch(html, /Keeps this revision/);
});

test("renders the comparison's per-row note after the relation text", () => {
  const html = renderToStaticMarkup(
    React.createElement(RolePackSnapshots, {
      snapshots: {
        resolvedAt: null,
        resolved: [],
        reported: [
          reportedRow({
            relation: "unrelated",
            note: "this machine's packs checkout is shallow, so ancestry cannot be ranked",
          }),
        ],
        comparison: null,
        provenanceNotes: [],
      },
      resolvedError: null,
      resolvedIsStale: false,
      revisionsError: null,
      reports: { isLoading: false, error: null, authorityError: null },
    }),
  );

  // The relation text and the note render as sibling spans (each joined by
  // its own " · " separator), so they are matched independently rather than
  // as one contiguous run of text. The apostrophe is skipped entirely —
  // `renderToStaticMarkup` HTML-escapes it to `&#x27;`.
  assert.match(html, /Different history from this machine/);
  assert.match(html, /packs checkout is shallow, so ancestry cannot be ranked/);
});

test("omits the note segment entirely when the row carries none", () => {
  const html = renderToStaticMarkup(
    React.createElement(RolePackSnapshots, {
      snapshots: {
        resolvedAt: null,
        resolved: [],
        reported: [reportedRow({ relation: "current", note: null })],
        comparison: null,
        provenanceNotes: [],
      },
      resolvedError: null,
      resolvedIsStale: false,
      revisionsError: null,
      reports: { isLoading: false, error: null, authorityError: null },
    }),
  );

  assert.match(html, /Same revision as this machine/);
  assert.match(html, /reported 5m ago/);
  assert.doesNotMatch(html, /checkout is shallow/);
});

test("does not turn a failed session discovery into a zero-report claim", () => {
  const html = renderToStaticMarkup(
    React.createElement(RolePackSnapshots, {
      snapshots: {
        resolvedAt: null,
        resolved: [],
        reported: [],
        comparison: null,
        provenanceNotes: [],
      },
      resolvedError: null,
      resolvedIsStale: false,
      revisionsError: null,
      reports: {
        isLoading: false,
        error: "could not read this project's channels",
        authorityError: null,
      },
    }),
  );

  assert.match(html, /Metadata claims may be incomplete/);
  assert.match(html, /No complete metadata-claim list is available/);
  assert.doesNotMatch(
    html,
    /No role-version metadata claims are visible for this project/,
  );
});

// ── Provenance labels ────────────────────────────────────────────────────

function renderReported(reported) {
  return renderToStaticMarkup(
    React.createElement(RolePackSnapshots, {
      snapshots: {
        resolvedAt: null,
        resolved: [],
        reported,
        comparison: null,
        provenanceNotes: [],
      },
      resolvedError: null,
      resolvedIsStale: false,
      revisionsError: null,
      reports: { isLoading: false, error: null, authorityError: null },
    }),
  );
}

test("a commissioned row carries the final wording, a plain (non-error) chip, and no reason suffix", () => {
  const founder = "c".repeat(64);
  const html = renderReported([
    reportedRow({
      provenance: "commissioned",
      provenanceReason: null,
      founderPubkey: founder,
    }),
  ]);

  assert.match(html, /Reported by the assigned provider/);
  // `commissioned` never carries a `· <reason>` suffix — the type contract
  // fixes `reason: null` for it.
  assert.doesNotMatch(html, /Reported by the assigned provider ·/);
  // Never word it as verified execution or adoption (constraint 2).
  assert.doesNotMatch(
    html,
    /verified execution|verified adoption|ran the pack/,
  );
  assert.match(html, /data-provenance="commissioned"/);
  // Plain, not amber or destructive — no error-toned class on the chip.
  assert.doesNotMatch(
    html,
    /text-amber-600[^"]*"[^>]*>Reported by the assigned provider/,
  );
});

test("a proof-unavailable row appends its reason and carries the amber chip", () => {
  const html = renderReported([
    reportedRow({
      provenance: "proof-unavailable",
      provenanceReason: "operator authority not projected",
    }),
  ]);

  assert.match(
    html,
    /Unverified · proof unavailable · operator authority not projected/,
  );
  assert.match(html, /data-provenance="proof-unavailable"/);
  assert.match(html, /text-amber-600[^>]*>Unverified · proof unavailable/);
});

test("a disputed row appends its reason and carries the destructive chip", () => {
  const html = renderReported([
    reportedRow({
      provenance: "disputed",
      provenanceReason: "the 44223 signer is not the accepted provider",
    }),
  ]);

  assert.match(
    html,
    /Disputed · the 44223 signer is not the accepted provider/,
  );
  assert.match(html, /data-provenance="disputed"/);
  assert.match(html, /text-destructive[^>]*>Disputed/);
});

test("every reported row keeps its data-provenance attribute alongside data-relation", () => {
  const html = renderReported([
    reportedRow({ relation: "earlier", behind: 2, provenance: "disputed" }),
  ]);

  assert.match(html, /data-relation="earlier"/);
  assert.match(html, /data-provenance="disputed"/);
});

test("the reported heading surfaces the provenance fold's notes as one 'Proof reads' line", () => {
  const html = renderToStaticMarkup(
    React.createElement(RolePackSnapshots, {
      snapshots: {
        resolvedAt: null,
        resolved: [],
        reported: [reportedRow()],
        comparison: null,
        provenanceNotes: ["Lifecycle receipts could not be read: timed out."],
      },
      resolvedError: null,
      resolvedIsStale: false,
      revisionsError: null,
      reports: { isLoading: false, error: null, authorityError: null },
    }),
  );

  assert.match(
    html,
    /Proof reads: Lifecycle receipts could not be read: timed out\./,
  );
});

test("the 'Proof reads' line is omitted entirely when there are no provenance notes", () => {
  const html = renderReported([reportedRow()]);
  assert.doesNotMatch(html, /Proof reads:/);
});

test("large histories render a bounded page while disclosing the full count", () => {
  const html = renderToStaticMarkup(
    React.createElement(RolePackSnapshots, {
      snapshots: {
        resolvedAt: null,
        resolved: [],
        comparison: null,
        provenanceNotes: [],
        reported: Array.from({ length: 1000 }, (_, i) =>
          reportedRow({ generationId: `generation-${i}` }),
        ),
      },
      resolvedError: null,
      resolvedIsStale: false,
      revisionsError: null,
      reports: { isLoading: false, error: null, authorityError: null },
    }),
  );
  assert.equal(
    (html.match(/data-testid="role-pack-reported-row"/g) ?? []).length,
    25,
  );
  assert.match(html, /of 1000 reports/);
  assert.match(html, /Report history pages/);
});

test("history navigation reaches later reports and returns without growing the DOM", async () => {
  const { JSDOM } = await import("jsdom");
  const dom = new JSDOM("<div id='root'></div>");
  const oldWindow = globalThis.window;
  const oldDocument = globalThis.document;
  const oldAct = globalThis.IS_REACT_ACT_ENVIRONMENT;
  globalThis.window = dom.window;
  globalThis.document = dom.window.document;
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const { createRoot } = await import("react-dom/client");
  const root = createRoot(document.getElementById("root"));
  try {
    await React.act(async () =>
      root.render(
        React.createElement(RolePackSnapshots, {
          snapshots: {
            resolvedAt: null,
            resolved: [],
            comparison: null,
            provenanceNotes: [],
            reported: Array.from({ length: 51 }, (_, i) =>
              reportedRow({
                generationId: `g-${i}`,
                label: `Report number ${i}`,
              }),
            ),
          },
          resolvedError: null,
          resolvedIsStale: false,
          revisionsError: null,
          reports: { isLoading: false, error: null, authorityError: null },
        }),
      ),
    );
    const rows = () =>
      document.querySelectorAll('[data-testid="role-pack-reported-row"]');
    const next = () =>
      [...document.querySelectorAll("button")].find(
        (b) => b.textContent === "Next",
      );
    assert.equal(rows().length, 25);
    await React.act(async () => next().click());
    assert.match(rows()[0].textContent, /Report number 25/);
    await React.act(async () => next().click());
    assert.equal(rows().length, 1);
    assert.equal(next().disabled, true);
    await React.act(async () =>
      [...document.querySelectorAll("button")]
        .find((b) => b.textContent === "Previous")
        .click(),
    );
    assert.equal(rows().length, 25);
  } finally {
    await React.act(async () => root.unmount());
    dom.window.close();
    globalThis.window = oldWindow;
    globalThis.document = oldDocument;
    globalThis.IS_REACT_ACT_ENVIRONMENT = oldAct;
  }
});

// ── Technical details ────────────────────────────────────────────────────

test("everything is inside a Technical details disclosure that is collapsed by default", () => {
  const html = renderToStaticMarkup(
    React.createElement(RolePackSnapshots, {
      snapshots: {
        resolvedAt: 1_700_000_000_000,
        resolved: [{ role: "builder", coordinate, reason: null }],
        reported: [reportedRow(), reportedRow({ generationId: "g-2" })],
        comparison: null,
        provenanceNotes: [],
      },
      resolvedError: null,
      resolvedIsStale: false,
      revisionsError: null,
      reports: { isLoading: false, error: null, authorityError: null },
    }),
  );

  // One outer disclosure, three named groups, none of them open.
  assert.match(html, /<details data-testid="roles-technical-details">/);
  assert.match(html, /<summary[^>]*>Technical details<\/summary>/);
  assert.match(html, /<details[^>]*data-testid="roles-diagnostics-source"/);
  assert.match(html, /<details[^>]*data-testid="roles-diagnostics-history"/);
  assert.match(html, /<details[^>]*data-testid="roles-diagnostics-checks"/);
  assert.doesNotMatch(html, /<details[^>]*data-testid="roles-[^"]*"[^>]*open/);
  // Each group is indented off the same rule, so the three read as one list.
  assert.equal(
    (html.match(/border-l border-border\/60 pl-3/g) ?? []).length,
    3,
  );

  // The named group summaries, including the history's own count.
  assert.match(html, /Where instructions come from/);
  assert.match(html, /Report history \(2\)/);
  assert.match(html, /How Beekeeper checks reports/);
  // The page's own explanation of what a role is lives here now, verbatim.
  assert.match(html, /data-testid="roles-explanation"/);
  assert.match(html, /Each role is a set of instructions an agent follows/);

  // The rows themselves are unchanged and still inside the groups.
  assert.match(html, /data-testid="role-pack-resolved"/);
  assert.match(html, /data-testid="role-pack-resolved-row"/);
  assert.match(html, /data-testid="role-pack-reported"/);
  assert.equal(
    (html.match(/data-testid="role-pack-reported-row"/g) ?? []).length,
    2,
  );
});

test("the checks group carries the subtitle, the proof reads and the sessions notice", () => {
  const html = renderToStaticMarkup(
    React.createElement(RolePackSnapshots, {
      snapshots: {
        resolvedAt: null,
        resolved: [],
        reported: [],
        comparison: null,
        provenanceNotes: ["Lifecycle receipts could not be read: timed out."],
      },
      resolvedError: null,
      resolvedIsStale: false,
      revisionsError: null,
      reports: { isLoading: false, error: null, authorityError: null },
      shelfNotice: {
        kind: "partial",
        message: "Some sessions could not be read.",
        detail: "one channel did not answer",
      },
    }),
  );

  assert.match(
    html,
    /Beekeeper checks who sent each report\. Confirming its source does not prove which role instructions were used\./,
  );
  assert.match(html, /data-testid="role-pack-provenance-notes"/);
  assert.match(html, /Proof reads: Lifecycle receipts could not be read/);
  assert.match(html, /data-testid="roles-shelf-notice"/);
  assert.match(html, /data-shelf-state="partial"/);
  assert.match(
    html,
    /Some sessions could not be read\. — one channel did not answer/,
  );
});

test("no sessions notice renders when the shelf read nothing to disclose", () => {
  const html = renderReported([reportedRow()]);
  assert.doesNotMatch(html, /data-testid="roles-shelf-notice"/);
});
