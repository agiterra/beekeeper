import assert from "node:assert/strict";
import test from "node:test";

import {
  filePathChipLabel,
  inlineCodeFilePathCandidate,
  markdownHrefFilePathCandidate,
  splitFilePathPosition,
} from "./filePathCandidate.ts";

// T3's table (packages/client-runtime/src/markdownLinks.test.ts, t3code
// 517188b3), less its absolute rows, plus the SV-32 acceptance cases.
const INLINE_CASES = [
  ["src\\main.ts", "src/main.ts"],
  ["conf.d/nginx.conf", "conf.d/nginx.conf"],
  ["script.pl:10", "script.pl:10"],
  ["node.meta", null],
  ["Recorded evidence here: /tmp/image.png", null],
  ["origin/main", null],
  ["127.0.0.1:3000", null],
  ["example.com/index.html", null],
  ["example.com/x", null],
  ["example.pl/index.html", null],
  ["z-ai/glm-5.3", null],
  ["z-ai/glm-5.3:12", null],
  ["python/3.12", null],
  ["Qwen/Qwen2.5-Coder", null],
  ["meta-llama/Llama-3.1-8B", null],
  ["share/man/ls.1", "share/man/ls.1"],
  ["usr/lib/libfoo.so.1", "usr/lib/libfoo.so.1"],
  ["vendor/jquery-3.6.0.min.js", "vendor/jquery-3.6.0.min.js"],
  ["./models/glm-5.3", "./models/glm-5.3"],
  ["v1.2.3", null],
  ["desktop/src/app/App.tsx:42", "desktop/src/app/App.tsx:42"],
  ["../plans/x.md", "../plans/x.md"],
  ["README.md:3", "README.md:3"],
  ["README.md", null],
  ["localhost:3000", null],
  // Beekeeper difference: absolute paths are never candidates (they are
  // elided before signing; vault recovery is S3).
  ["/Users/x/a.md", null],
  ["~/notes/a.md", null],
  ["C:\\Users\\demo\\image.png", null],
  ["\\\\server\\share\\image.png", null],
  ["", null],
  ["`a/b.ts`", null],
];

for (const [source, expected] of INLINE_CASES) {
  test(`inline code ${JSON.stringify(source)} → ${JSON.stringify(expected)}`, () => {
    assert.equal(inlineCodeFilePathCandidate(source), expected);
  });
}

const HREF_CASES = [
  ["desktop/src/app/App.tsx", "desktop/src/app/App.tsx"],
  ["desktop/src/app/App.tsx#L42", "desktop/src/app/App.tsx:42"],
  ["src/main.ts#L18C2", "src/main.ts:18:2"],
  ["./plans/x.md", "./plans/x.md"],
  ["file%20name.md", "file name.md"],
  ["<docs/a b.md>", "docs/a b.md"],
  ["README.md", "README.md"],
  ["https://example.com/a.ts", null],
  ["file:///Users/x/a.ts", null],
  ["beekeeper://channel/abc", null],
  ["mailto:a@b.c", null],
  ["//cdn.example.com/a.js", null],
  ["/Users/x/a.md", null],
  ["#section", null],
  ["example.com/x", null],
  ["docs/a.md?raw=1", null],
  [undefined, null],
];

for (const [href, expected] of HREF_CASES) {
  test(`link href ${JSON.stringify(href)} → ${JSON.stringify(expected)}`, () => {
    assert.equal(markdownHrefFilePathCandidate(href), expected);
  });
}

test("splitFilePathPosition mirrors the host's :line[:col] split", () => {
  assert.deepEqual(splitFilePathPosition("src/main.ts"), {
    path: "src/main.ts",
  });
  assert.deepEqual(splitFilePathPosition("src/main.ts:12"), {
    path: "src/main.ts",
    line: 12,
  });
  assert.deepEqual(splitFilePathPosition("src/main.ts:12:5"), {
    path: "src/main.ts",
    line: 12,
    column: 5,
  });
  assert.deepEqual(splitFilePathPosition("src/main.ts:0"), {
    path: "src/main.ts",
  });
});

test("the chip label is the basename and the position", () => {
  assert.equal(
    filePathChipLabel("desktop/src/app/App.tsx:42"),
    "App.tsx · L42",
  );
  assert.equal(filePathChipLabel("src/main.ts:12:3"), "main.ts · L12:C3");
  assert.equal(filePathChipLabel("plans/x.md"), "x.md");
  assert.equal(filePathChipLabel("desktop/src/"), "src");
});
