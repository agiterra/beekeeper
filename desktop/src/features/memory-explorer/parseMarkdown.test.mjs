import assert from "node:assert/strict";
import test from "node:test";
import { parseMarkdown } from "./parseMarkdown.ts";
import { connect, localTarget } from "./model.ts";

test("generic Markdown: GFM escaped pipes, definitions, duplicate anchors and fenced numbers", () => {
  const text =
    "# Notes\n\n[Next][next]\n\n[next]: nested/file.md#hello\n\n## Same\n\nText\n\n## Same\n\n| topic | state |\n| --- | --- |\n| a \\| b | uncertain |\n\n```md\n336. **not a finding**\nledger 999\n```\n";
  const doc = parseMarkdown("README.md", text);
  assert.deepEqual(
    doc.nodes.map((n) => n.fragment),
    ["", "notes", "same", "same-1"],
  );
  assert(
    doc.nodes.some((n) =>
      n.references.some((r) => r.target === "nested/file.md#hello"),
    ),
  );
  assert(!doc.nodes.some((n) => n.claim || n.type === "finding"));
  assert.equal(localTarget("nested/a.md", "../../secret.md"), null);
  assert.equal(localTarget("nested/a.md", "https://example.org"), null);
});

test("Beekeeper enrichment retains literal claims, exact findings and candidate IDs", () => {
  const ledger = parseMarkdown(
    "plans/SESSION_STATE.md",
    "## 2. Open — findings\n\n336. **Old observation SV-77.**\n    literal source.\n\n340. **Fixed SV-77; fixes 336; new SV-88..SV-90.**\n    installed proof owed (SV-89).\n\n341. **Installed proof closes SV-89.**\n\n```md\n999. **example**\n```\n",
    true,
  );
  const plan = parseMarkdown(
    "plans/parity.md",
    "| ID | Item | Evidence | Status |\n| --- | --- | --- | --- |\n| SV-77 | Wake | ledger 336 | landed; proof owed (ledger 340) |\n| SV-89 | Prove | ledger 340 | closed (ledger 341) |\n",
    true,
  );
  const duplicate = parseMarkdown(
    "plans/other.md",
    "| ID | Item | Status |\n| --- | --- | --- |\n| SV-89 | Other | unknown wording |\n",
    true,
  );
  assert.equal(ledger.nodes.find((n) => n.fragment === "ledger-336").start, 3);
  assert(
    ledger.nodes
      .find((n) => n.fragment === "ledger-336")
      .text.startsWith("336."),
  );
  assert(
    ledger.nodes
      .find((n) => n.fragment === "ledger-336")
      .readText.startsWith("**Old observation"),
  );
  assert(!ledger.nodes.some((n) => n.fragment === "ledger-999"));
  assert.equal(
    plan.nodes.find((n) => n.fragment === "SV-77").claim,
    "landed; proof owed (ledger 340)",
  );
  const edges = connect([ledger, plan, duplicate]);
  assert(
    edges.some(
      (e) =>
        e.from === "plans/parity.md#SV-77" &&
        e.to.includes("plans/SESSION_STATE.md#ledger-336"),
    ),
  );
  assert(
    edges.some(
      (e) =>
        e.from === "plans/SESSION_STATE.md#ledger-340" &&
        e.target === "sv:SV-89" &&
        e.to.length === 2,
    ),
  );
  assert(
    edges.some(
      (e) =>
        e.from === "plans/parity.md#SV-89" &&
        e.to.includes("plans/SESSION_STATE.md#ledger-341"),
    ),
  );
});

test("ledger examples outside findings and code are not identities; raw claim bytes survive", () => {
  const doc = parseMarkdown(
    "plans/SESSION_STATE.md",
    "## 2. Open — findings\n\n23. **Real.**\n\n## 3. Next\n\n1. **Example list.**\n\n## 5. The rule\n\n24. **Appended finding.**\n",
    true,
  );
  assert(doc.nodes.some((n) => n.fragment === "ledger-23"));
  assert(doc.nodes.some((n) => n.fragment === "ledger-24"));
  assert(!doc.nodes.some((n) => n.fragment === "ledger-1"));
  const plan = parseMarkdown(
    "plans/parity.md",
    "| ID | Item | Status |\n| --- | --- | --- |\n| SV-1 | Thing | landed `abcdef` — proof owed |\n",
    true,
  );
  assert.equal(
    plan.nodes.find((n) => n.fragment === "SV-1").claim,
    "landed `abcdef` — proof owed",
  );
});

test("frontmatter is preserved separately and giant plain documents have complete section coverage", () => {
  const doc = parseMarkdown(
    "notes.md",
    "---\nschema: custom\n---\n" +
      Array.from({ length: 1500 }, (_, i) => `line ${i}`).join("\n"),
  );
  assert.equal(doc.frontmatter, "---\nschema: custom\n---");
  assert(doc.nodes.some((n) => n.text.includes("line 1499")));
  assert(doc.nodes.every((n) => n.end - n.start <= 400));
});

test("finding Read removes container indentation without altering exact source or nested code", () => {
  const text =
    "## 2. Open — findings\n\n340. **Observation.**\n     Prose continues.\n\n     Another paragraph.\n\n     ```sh\n     echo literal\n     ```\n";
  const finding = parseMarkdown(
    "plans/SESSION_STATE.md",
    text,
    true,
  ).nodes.find((n) => n.fragment === "ledger-340");
  assert(finding);
  assert(finding.text.includes("\n     Another paragraph."));
  assert(finding.readText.includes("\n\nAnother paragraph."));
  assert(finding.readText.includes("\n```sh\necho literal\n```"));
});
