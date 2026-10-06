import assert from "node:assert/strict";
import test from "node:test";

import remarkRedactionMarkers from "./remarkRedactionMarkers.ts";

const DIGEST =
  "eb7930a9a9209e69d829efa946d4ebea3f2e32c6b03f3317321c53bf33597e3f";
const MARKER = `[elided private context: 148 bytes, sha256:${DIGEST}]`;

function paragraph(value) {
  const tree = {
    type: "root",
    children: [{ type: "paragraph", children: [{ type: "text", value }] }],
  };
  remarkRedactionMarkers()(tree);
  return tree.children[0].children;
}

test("a marker mid-paragraph becomes a redaction node between text nodes", () => {
  const children = paragraph(`The result was ${MARKER}, so I stopped.`);

  assert.deepEqual(children[0], { type: "text", value: "The result was " });
  assert.equal(children[1].type, "redaction");
  assert.equal(children[1].data.hName, "redaction");
  assert.deepEqual(children[1].data.hProperties, {
    "data-redaction-bytes": "148",
    "data-redaction-digest": DIGEST,
  });
  assert.deepEqual(children[2], { type: "text", value: ", so I stopped." });
});

test("the redaction node has no children — the pill renders its own label", () => {
  const [node] = paragraph(MARKER);
  assert.deepEqual(node.data.hChildren, []);
});

test("two markers in one paragraph both convert", () => {
  const other = `[elided private context: 9 bytes, sha256:${"a".repeat(64)}]`;
  const children = paragraph(`${MARKER} and ${other}`);
  const redactions = children.filter((child) => child.type === "redaction");
  assert.equal(redactions.length, 2);
  assert.equal(redactions[1].data.hProperties["data-redaction-bytes"], "9");
});

test("a fenced code block keeps the literal marker", () => {
  // Inside a fence the reader is looking at bytes, and a pill would claim the
  // bytes say something they do not.
  const tree = {
    type: "root",
    children: [{ type: "code", value: `echo ${MARKER}` }],
  };
  remarkRedactionMarkers()(tree);
  assert.equal(tree.children[0].value, `echo ${MARKER}`);
});

test("inline code keeps the literal marker", () => {
  const tree = {
    type: "root",
    children: [
      {
        type: "paragraph",
        children: [
          { type: "inlineCode", children: [{ type: "text", value: MARKER }] },
        ],
      },
    ],
  };
  remarkRedactionMarkers()(tree);
  const inline = tree.children[0].children[0];
  assert.equal(inline.type, "inlineCode");
  assert.deepEqual(inline.children, [{ type: "text", value: MARKER }]);
});

test("prose with no marker is left structurally untouched", () => {
  const children = paragraph("ran the tests and they passed");
  assert.deepEqual(children, [
    { type: "text", value: "ran the tests and they passed" },
  ]);
});

test("a malformed marker is left as text", () => {
  const truncated = "[elided private context: 148 bytes, sha256:beef]";
  assert.deepEqual(paragraph(truncated), [{ type: "text", value: truncated }]);
});

test("plugin instances do not share regex state", () => {
  // Two renders can be interleaved; a `g`-flagged regex shared across them
  // would skip matches based on the other's `lastIndex`.
  const first = remarkRedactionMarkers();
  const second = remarkRedactionMarkers();
  for (const plugin of [first, second, first, second]) {
    const tree = {
      type: "root",
      children: [
        { type: "paragraph", children: [{ type: "text", value: MARKER }] },
      ],
    };
    plugin(tree);
    assert.equal(tree.children[0].children[0].type, "redaction");
  }
});

test("a marker touching a path separator is flagged as a path", () => {
  const children = paragraph(`Edited ${MARKER}/src/main.rs today.`);
  assert.equal(children[1].type, "redaction");
  assert.equal(children[1].data.hProperties["data-redaction-path"], "true");
});

test("a marker in ordinary prose is not flagged as a path", () => {
  const children = paragraph(`The result was ${MARKER}, so I stopped.`);
  assert.equal(children[1].data.hProperties["data-redaction-path"], undefined);
});
