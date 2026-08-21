/**
 * H-08 implementation binder — runs the REAL Buzz transcript-export engine
 * against every banked fixture vector in this corpus. `fixtures.test.mjs`
 * keeps its own independent re-implementations of the donor laws; this file
 * binds the production code to the same vectors, so the two suites
 * triangulate (law vs implementation) and can never drift apart silently.
 *
 * Import constraint (load-bearing): the engine module must stay free of
 * runtime imports and use erasable TypeScript syntax only, because this file
 * runs under plain `node --test` (no loader) relying on Node 24's native
 * type stripping. If that constraint breaks, this suite fails loudly with a
 * module-load error — never silently.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  SHARE_WORKSPACE_PATH,
  TRANSCRIPT_BUNDLE_VERSION,
  applyAttachmentMode,
  buildTranscriptExportPlan,
  formatExportTimestamp,
  resolveUniqueExportDirName,
  rewriteLocalPathsForShare,
  sanitizeFileNameSegment,
} from "../../desktop/src/features/coding-sessions/lib/transcriptExport/transcriptExportEngine.ts";
import {
  buildManifest,
  cacheControlFor,
  contentTypeFor,
  deriveReleaseTag,
  toReleaseAssetName,
} from "../../scripts/export-viewer-release-manifest.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const load = (name) =>
  JSON.parse(readFileSync(path.join(HERE, "fixtures", name), "utf8"));
const BUNDLE = load("bundle-vectors.json");
const NAMING = load("naming-vectors.json");
const MANIFEST = load("release-manifest-vectors.json");

test("bundle constants bind the Buzz engine", () => {
  assert.equal(TRANSCRIPT_BUNDLE_VERSION, BUNDLE.bundle_constants.version);
  assert.equal(SHARE_WORKSPACE_PATH, BUNDLE.bundle_constants.sharePath);
});

test("attachment mode matrix binds the Buzz engine", () => {
  for (const vector of BUNDLE.attachment_cases) {
    const derived = applyAttachmentMode(
      vector.attachment,
      vector.mode,
      vector.attachment.sourceExists,
    );
    assert.deepEqual(
      { ...derived.fields, bundled: derived.bundled },
      vector.expected,
      vector.name,
    );
  }
});

test("share-path rewrite vectors bind the Buzz engine", () => {
  for (const vector of BUNDLE.path_rewrite_cases) {
    assert.deepEqual(
      rewriteLocalPathsForShare(vector.input, vector.workspacePath),
      vector.expected,
      vector.name,
    );
  }
});

test("a Buzz-built bundle never contains the workspace path and pins localPath", () => {
  const workspacePath = "/Users/op/secret project";
  const plan = buildTranscriptExportPlan({
    chatId: "chat-1",
    title: "Review",
    workspacePath,
    theme: "light",
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
        attachments: [
          {
            id: "a",
            displayName: "x.png",
            mimeType: "image/png",
            size: 1,
            absolutePath: `${workspacePath}/x.png`,
            relativePath: "x.png",
            contentUrl: "asset://x.png",
          },
        ],
      },
    ],
    nowIso: "2026-04-23T12:34:56.000Z",
    viewerVersion: "1.0.0",
    takenDirectoryNames: [],
    attachmentSourceExists: {},
  });
  const bundle = JSON.parse(plan.transcriptJson);
  assert.equal(bundle.localPath, SHARE_WORKSPACE_PATH);
  assert.ok(!plan.transcriptJson.includes(workspacePath));
});

test("segment sanitization vectors bind the Buzz engine", () => {
  for (const vector of NAMING.sanitize_cases) {
    assert.equal(
      sanitizeFileNameSegment(vector.input),
      vector.expected,
      JSON.stringify(vector.input),
    );
  }
});

test("export timestamp vectors bind the Buzz engine", () => {
  for (const vector of NAMING.timestamp_cases) {
    assert.equal(formatExportTimestamp(vector.input), vector.expected, vector.input);
  }
});

test("unique export directory vectors bind the Buzz engine", () => {
  for (const vector of NAMING.directory_cases) {
    assert.equal(
      resolveUniqueExportDirName(
        vector.title,
        vector.chatId,
        vector.timestamp,
        new Set(vector.taken),
      ),
      vector.expected,
      vector.name,
    );
  }
});

test("release asset name vectors bind the Buzz manifest script", () => {
  for (const vector of MANIFEST.asset_name_cases) {
    assert.equal(toReleaseAssetName(vector.input), vector.expected, vector.input);
  }
});

test("cache control vectors bind the Buzz manifest script", () => {
  for (const vector of MANIFEST.cache_control_cases) {
    assert.equal(cacheControlFor(vector.input), vector.expected, vector.input);
  }
});

test("content type vectors bind the Buzz manifest script", () => {
  for (const vector of MANIFEST.content_type_cases) {
    assert.equal(contentTypeFor(vector.input), vector.expected, vector.input);
  }
});

test("release tag vectors bind the Buzz manifest script", () => {
  for (const vector of MANIFEST.release_tag_cases) {
    assert.equal(deriveReleaseTag(vector.input), vector.expected, vector.input);
  }
});

test("the frozen manifest shape binds the Buzz manifest script", () => {
  const manifest = buildManifest("1.2.3", ["index.html"], "2026-08-06T00:00:00.000Z");
  assert.deepEqual(Object.keys(manifest), MANIFEST.manifest_shape.top_level_keys);
  assert.deepEqual(
    Object.keys(manifest.files["index.html"]),
    MANIFEST.manifest_shape.per_file_keys,
  );
});
