/**
 * H-08 — Buzz transcript-export engine tests.
 *
 * The banked contract lives in `conformance/transcript-export/` (donor: Hive
 * `src/server/standalone-export.ts` at pin `e0b8198bd144`). This suite
 * restates the corpus vectors as self-contained values (upstream-owned tests
 * do not read the fork-owned conformance tree); the conformance bridge test
 * (`conformance/transcript-export/implementation.test.mjs`) additionally runs
 * the engine against the fixture files themselves, so the two suites
 * triangulate. Every expected value here is re-derived from the banked law,
 * never from executing donor code.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  SHARE_WORKSPACE_PATH,
  TRANSCRIPT_BUNDLE_VERSION,
  applyAttachmentMode,
  buildTranscriptExportPlan,
  formatExportTimestamp,
  resolveUniqueExportDirName,
  rewriteLocalPathsForShare,
  sanitizeFileNameSegment,
} from "./transcriptExportEngine.ts";

function attachment(overrides = {}) {
  return {
    id: "att-1",
    displayName: "mock.png",
    mimeType: "image/png",
    size: 1024,
    absolutePath: "/ws/.hive/uploads/mock.png",
    relativePath: ".hive/uploads/mock.png",
    contentUrl: "asset://localhost/mock.png",
    ...overrides,
  };
}

test("segment sanitization: trim, collapse runs outside [A-Za-z0-9_.-], strip edge dashes", () => {
  const vectors = [
    ["Release Review", "Release-Review"],
    ["  spaced out  ", "spaced-out"],
    ["a/b\\c", "a-b-c"],
    ["üñïçode", "ode"],
    ["---x---", "x"],
    ["...dots...", "...dots..."],
    ["!!!", ""],
    ["", ""],
  ];
  for (const [input, expected] of vectors) {
    assert.equal(
      sanitizeFileNameSegment(input),
      expected,
      JSON.stringify(input),
    );
  }
});

test("export timestamp: colons to dashes, milliseconds stripped", () => {
  assert.equal(
    formatExportTimestamp("2026-04-23T12:34:56.000Z"),
    "2026-04-23T12-34-56Z",
  );
  assert.equal(
    formatExportTimestamp("2026-12-31T23:59:59.999Z"),
    "2026-12-31T23-59-59Z",
  );
});

test("unique export directory: title falls to chatId, empty-sanitized falls to the chat literal, collisions suffix", () => {
  const ts = "2026-04-23T12:34:56.000Z";
  assert.equal(
    resolveUniqueExportDirName("Release Review", "chat-1", ts, new Set()),
    "Release-Review-2026-04-23T12-34-56Z",
  );
  assert.equal(
    resolveUniqueExportDirName("", "chat 9!", ts, new Set()),
    "chat-9-2026-04-23T12-34-56Z",
  );
  // A truthy title that sanitizes to nothing falls to the literal "chat",
  // NOT to chatId — the banked sharp edge.
  assert.equal(
    resolveUniqueExportDirName("!!!", "chat-1", ts, new Set()),
    "chat-2026-04-23T12-34-56Z",
  );
  assert.equal(
    resolveUniqueExportDirName(
      "Release Review",
      "chat-1",
      ts,
      new Set(["Release-Review-2026-04-23T12-34-56Z"]),
    ),
    "Release-Review-2026-04-23T12-34-56Z-2",
  );
  assert.equal(
    resolveUniqueExportDirName(
      "Release Review",
      "chat-1",
      ts,
      new Set([
        "Release-Review-2026-04-23T12-34-56Z",
        "Release-Review-2026-04-23T12-34-56Z-2",
      ]),
    ),
    "Release-Review-2026-04-23T12-34-56Z-3",
  );
});

test("attachment matrix: metadata mode empties all three reference fields", () => {
  const derived = applyAttachmentMode(attachment(), "metadata", true);
  assert.deepEqual(derived.fields, {
    absolutePath: "",
    relativePath: "",
    contentUrl: "",
  });
  assert.equal(derived.bundled, false);
  assert.equal(derived.exportedFileName, null);
});

test("attachment matrix: bundle mode rewrites all three fields to one sanitized attachments path", () => {
  const derived = applyAttachmentMode(
    attachment({
      id: "att 1!",
      displayName: "shot (final).png",
      absolutePath: "/ws/.hive/uploads/shot.png",
    }),
    "bundle",
    true,
  );
  const rewritten = "./attachments/att-1-shot-final-.png";
  assert.deepEqual(derived.fields, {
    absolutePath: rewritten,
    relativePath: rewritten,
    contentUrl: rewritten,
  });
  assert.equal(derived.bundled, true);
  assert.equal(derived.exportedFileName, "att-1-shot-final-.png");
});

test("attachment matrix: bundle mode falls back to the metadata rewrite on missing or path-less sources", () => {
  for (const candidate of [
    applyAttachmentMode(attachment({ id: "att-2" }), "bundle", false),
    applyAttachmentMode(
      attachment({ id: "att-3", absolutePath: "" }),
      "bundle",
      false,
    ),
  ]) {
    assert.deepEqual(candidate.fields, {
      absolutePath: "",
      relativePath: "",
      contentUrl: "",
    });
    assert.equal(candidate.bundled, false);
    assert.equal(candidate.exportedFileName, null);
  }
});

test("attachment matrix: bundle mode derives the copied name from absolutePath when displayName is empty", () => {
  const derived = applyAttachmentMode(
    attachment({
      id: "att-4",
      displayName: "",
      absolutePath: "/ws/.hive/uploads/raw name.bin",
    }),
    "bundle",
    true,
  );
  assert.equal(derived.exportedFileName, "att-4-raw-name.bin");
  assert.equal(derived.fields.contentUrl, "./attachments/att-4-raw-name.bin");
});

test("share rewrite traverses strings, arrays, and objects; non-strings pass through", () => {
  assert.deepEqual(
    rewriteLocalPathsForShare(
      {
        kind: "assistant_text",
        text: "Wrote /Users/dev/proj x/src/a.ts and /Users/dev/proj x/b.ts",
      },
      "/Users/dev/proj x",
    ),
    {
      kind: "assistant_text",
      text: "Wrote /workspace/src/a.ts and /workspace/b.ts",
    },
  );
  assert.deepEqual(
    rewriteLocalPathsForShare(
      {
        steps: [{ cmd: "ls /home/op/work" }, 42, null, ["/home/op/work/x"]],
        count: 2,
      },
      "/home/op/work",
    ),
    { steps: [{ cmd: "ls /workspace" }, 42, null, ["/workspace/x"]], count: 2 },
  );
});

test("share rewrite with an empty workspace path rewrites nothing", () => {
  const input = { text: "untouched /anything" };
  assert.deepEqual(rewriteLocalPathsForShare(input, ""), input);
});

test("share rewrite is pure and rewrites a top-level string — the donor's discarded-return shape is not copied", () => {
  // Donor subtlety (recorded in the corpus contract): the donor mutates in
  // place and discards the return value, so a top-level STRING argument would
  // silently survive unrewritten. The Buzz engine returns the rewritten value
  // and the caller uses it.
  assert.equal(
    rewriteLocalPathsForShare("open /home/op/work/x.ts", "/home/op/work"),
    "open /workspace/x.ts",
  );
  const nested = { text: "/home/op/work" };
  const rewritten = rewriteLocalPathsForShare(nested, "/home/op/work");
  assert.notEqual(rewritten, nested);
  assert.equal(nested.text, "/home/op/work");
});

test("bundle plan: envelope shape is frozen, localPath pins the placeholder, nothing leaks the workspace path", () => {
  const workspacePath = "/Users/op/secret project";
  const plan = buildTranscriptExportPlan({
    chatId: "chat-1",
    title: "Review",
    workspacePath,
    theme: "dark",
    attachmentMode: "metadata",
    messages: [
      {
        kind: "assistant_text",
        id: "m-1",
        text: `Edited ${workspacePath}/src/main.rs`,
        timestamp: "2026-04-23T12:00:00.000Z",
      },
      {
        kind: "user_prompt",
        id: "m-2",
        text: "see attachment",
        timestamp: "2026-04-23T12:01:00.000Z",
        attachments: [attachment()],
      },
    ],
    nowIso: "2026-04-23T12:34:56.000Z",
    viewerVersion: "1.2.3",
    takenDirectoryNames: [],
    attachmentSourceExists: {},
  });

  const bundle = JSON.parse(plan.transcriptJson);
  assert.deepEqual(Object.keys(bundle), [
    "version",
    "chatId",
    "title",
    "localPath",
    "exportedAt",
    "viewerVersion",
    "theme",
    "attachmentMode",
    "messages",
  ]);
  assert.equal(bundle.version, TRANSCRIPT_BUNDLE_VERSION);
  assert.equal(bundle.localPath, SHARE_WORKSPACE_PATH);
  assert.equal(bundle.exportedAt, "2026-04-23T12:34:56.000Z");
  assert.equal(bundle.viewerVersion, "1.2.3");
  assert.equal(bundle.theme, "dark");
  assert.equal(bundle.attachmentMode, "metadata");
  assert.ok(!plan.transcriptJson.includes(workspacePath));
  assert.ok(plan.transcriptJson.endsWith("\n"));
  assert.equal(bundle.messages[0].text, "Edited /workspace/src/main.rs");
  assert.equal(plan.directoryName, "Review-2026-04-23T12-34-56Z");
});

test("bundle plan: counters count user_prompt attachments; only actual copies count as bundled", () => {
  const existing = attachment({
    id: "att-live",
    absolutePath: "/ws/up/live.png",
    displayName: "live.png",
  });
  const missing = attachment({
    id: "att-gone",
    absolutePath: "/ws/up/gone.pdf",
    displayName: "gone.pdf",
  });
  const plan = buildTranscriptExportPlan({
    chatId: "chat-1",
    title: "Counters",
    workspacePath: "/ws",
    theme: "light",
    attachmentMode: "bundle",
    messages: [
      {
        kind: "user_prompt",
        id: "m-1",
        text: "two files",
        timestamp: "2026-04-23T12:00:00.000Z",
        attachments: [existing, missing],
      },
    ],
    nowIso: "2026-04-23T12:34:56.000Z",
    viewerVersion: "0.1.0",
    takenDirectoryNames: [],
    attachmentSourceExists: {
      "/ws/up/live.png": true,
      "/ws/up/gone.pdf": false,
    },
  });

  assert.equal(plan.totalAttachmentCount, 2);
  assert.equal(plan.bundledAttachmentCount, 1);
  assert.deepEqual(plan.attachmentCopies, [
    {
      sourceAbsolutePath: "/ws/up/live.png",
      exportedFileName: "att-live-live.png",
    },
  ]);
  const bundle = JSON.parse(plan.transcriptJson);
  const [live, gone] = bundle.messages[0].attachments;
  assert.equal(live.contentUrl, "./attachments/att-live-live.png");
  assert.equal(live.absolutePath, "./attachments/att-live-live.png");
  assert.equal(gone.contentUrl, "");
  assert.equal(gone.absolutePath, "");
  // Metadata survives either way.
  assert.equal(gone.displayName, "gone.pdf");
  assert.equal(gone.mimeType, "image/png");
});

test("bundle plan: non-user_prompt messages pass through the attachment matrix untouched", () => {
  const plan = buildTranscriptExportPlan({
    chatId: "chat-1",
    title: "Pass through",
    workspacePath: "",
    theme: "light",
    attachmentMode: "bundle",
    messages: [
      {
        kind: "tool_call",
        id: "t-1",
        title: "Bash",
        toolName: "Bash",
        status: "completed",
        isError: false,
        text: "ok",
        timestamp: "2026-04-23T12:00:00.000Z",
      },
      {
        kind: "thought",
        id: "th-1",
        title: "Thinking",
        text: "hmm",
        timestamp: "2026-04-23T12:00:01.000Z",
      },
    ],
    nowIso: "2026-04-23T12:34:56.000Z",
    viewerVersion: "0.1.0",
    takenDirectoryNames: [],
    attachmentSourceExists: {},
  });
  assert.equal(plan.totalAttachmentCount, 0);
  assert.equal(plan.bundledAttachmentCount, 0);
  assert.deepEqual(plan.attachmentCopies, []);
  const bundle = JSON.parse(plan.transcriptJson);
  assert.deepEqual(
    bundle.messages.map((entry) => entry.kind),
    ["tool_call", "thought"],
  );
  assert.equal(bundle.messages[0].toolName, "Bash");
});

test("bundle plan: directory name respects taken names and the empty-workspace rewrite is a no-op", () => {
  const plan = buildTranscriptExportPlan({
    chatId: "chat-9",
    title: "",
    workspacePath: "",
    theme: "light",
    attachmentMode: "metadata",
    messages: [
      {
        kind: "assistant_text",
        id: "m-1",
        text: "untouched /anything",
        timestamp: "2026-04-23T12:00:00.000Z",
      },
    ],
    nowIso: "2026-04-23T12:34:56.000Z",
    viewerVersion: "0.1.0",
    takenDirectoryNames: ["chat-9-2026-04-23T12-34-56Z"],
    attachmentSourceExists: {},
  });
  assert.equal(plan.directoryName, "chat-9-2026-04-23T12-34-56Z-2");
  const bundle = JSON.parse(plan.transcriptJson);
  assert.equal(bundle.messages[0].text, "untouched /anything");
});
