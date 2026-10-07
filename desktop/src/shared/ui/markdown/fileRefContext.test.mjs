import assert from "node:assert/strict";
import { test } from "node:test";

import {
  FILE_REF_OUTSIDE_FOLDER_REASON,
  presentFileRef,
} from "./fileRefContext.ts";

const scope = (ref) => ({
  where: "thisComputer",
  reason: null,
  refs: { "notes.md": ref },
});
const ref = (relativePath) => ({
  exists: true,
  isDir: false,
  relativePath,
  fullPath: "/tmp/x/notes.md",
  line: null,
  column: null,
});

test("a path inside the folder is a chip", () => {
  assert.equal(presentFileRef(scope(ref("notes.md")), "notes.md").kind, "chip");
});

test("a path outside the folder is plain text saying so, never a chip", () => {
  assert.deepEqual(presentFileRef(scope(ref(null)), "notes.md"), {
    kind: "plain",
    reason: FILE_REF_OUTSIDE_FOLDER_REASON,
  });
});
