import assert from "node:assert/strict";
import test from "node:test";

import remarkPrivateContextMarkers from "./remarkPrivateContextMarkers.ts";

function render(value) {
  const tree = {
    type: "root",
    children: [{ type: "paragraph", children: [{ type: "text", value }] }],
  };
  remarkPrivateContextMarkers()(tree);
  return tree.children[0].children;
}

test("private context markers become compact chips with digest provenance", () => {
  const digest = "a".repeat(64);
  const children = render(
    `See [elided private context: 126 bytes, sha256:${digest}] for details.`,
  );
  assert.equal(children.length, 3);
  assert.equal(children[1].type, "private-context");
  assert.equal(children[1].data.hName, "private-context");
  assert.equal(
    children[1].data.hChildren[0].value,
    "Private context · 126 bytes",
  );
  assert.match(children[1].data.hProperties.title, new RegExp(digest));
});

test("malformed or short markers remain plain text", () => {
  const value = "[elided private context: 12 bytes, sha256:abc]";
  assert.deepEqual(render(value), [{ type: "text", value }]);
});
