/**
 * H-08 gate — re-derives every expected value in the transcript-export
 * fixtures with this file's own independent implementations of the donor
 * laws (bundle attachment matrix, share-path rewrite, naming, release
 * manifest). Self-contained: no shared helper module, no donor code
 * executed. Run: `node --test conformance/transcript-export/fixtures.test.mjs`.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const load = (name) => JSON.parse(readFileSync(path.join(HERE, "fixtures", name), "utf8"));
const BUNDLE = load("bundle-vectors.json");
const NAMING = load("naming-vectors.json");
const MANIFEST = load("release-manifest-vectors.json");

// ── Independent re-implementations of the donor laws ────────────────────────

const sanitizeSegment = (value) =>
  value.trim().replace(/[^\w.-]+/g, "-").replace(/^-+|-+$/g, "");

const exportTimestamp = (iso) => iso.replace(/:/g, "-").replace(/\.\d{3}Z$/u, "Z");

function uniqueExportDir(title, chatId, isoTimestamp, taken) {
  const base = `${sanitizeSegment(title || chatId) || "chat"}-${exportTimestamp(isoTimestamp)}`;
  const takenSet = new Set(taken);
  let candidate = base;
  for (let suffix = 2; takenSet.has(candidate); suffix += 1) {
    candidate = `${base}-${suffix}`;
  }
  return candidate;
}

function applyAttachmentMode(attachment, mode, sourceExists) {
  const emptied = { absolutePath: "", relativePath: "", contentUrl: "" };
  if (mode === "metadata") return { fields: emptied, bundled: false };
  if (!attachment.absolutePath || !sourceExists) return { fields: emptied, bundled: false };
  const base = path.posix.basename(attachment.displayName || attachment.absolutePath);
  const exportedName = `${sanitizeSegment(attachment.id)}-${sanitizeSegment(base)}`;
  const relative = `./attachments/${exportedName}`;
  return {
    fields: { absolutePath: relative, relativePath: relative, contentUrl: relative },
    bundled: true,
  };
}

function rewriteShare(value, workspacePath, sharePath) {
  if (!workspacePath) return value;
  if (typeof value === "string") return value.replaceAll(workspacePath, sharePath);
  if (Array.isArray(value)) return value.map((item) => rewriteShare(item, workspacePath, sharePath));
  if (!value || typeof value !== "object") return value;
  return Object.fromEntries(
    Object.entries(value).map(([key, nested]) => [key, rewriteShare(nested, workspacePath, sharePath)]),
  );
}

const assetName = (relative) => `export-viewer__${relative.split("/").join("__")}`;

const cacheControl = (relative) =>
  relative.endsWith(".html") ? "public, max-age=300" : "public, max-age=31536000, immutable";

const CONTENT_TYPES_BY_EXTENSION = {
  ".css": "text/css; charset=utf-8",
  ".gif": "image/gif",
  ".html": "text/html; charset=utf-8",
  ".ico": "image/x-icon",
  ".jpeg": "image/jpeg",
  ".jpg": "image/jpeg",
  ".js": "text/javascript; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".manifest": "application/manifest+json; charset=utf-8",
  ".mp3": "audio/mpeg",
  ".png": "image/png",
  ".svg": "image/svg+xml",
  ".txt": "text/plain; charset=utf-8",
  ".webmanifest": "application/manifest+json; charset=utf-8",
  ".webp": "image/webp",
  ".woff2": "font/woff2",
};

const contentType = (relative) =>
  CONTENT_TYPES_BY_EXTENSION[path.posix.extname(relative).toLowerCase()] ?? "application/octet-stream";

const releaseTag = (version) => `v${version.replace(/^v/u, "")}`;

// ── Provenance ──────────────────────────────────────────────────────────────

test("provenance pins the donor export surface", () => {
  for (const [fixture, donorPath] of [
    [BUNDLE, "src/server/standalone-export.ts"],
    [NAMING, "src/server/standalone-export.ts"],
    [MANIFEST, "scripts/prepare-export-viewer-release-assets.ts"],
  ]) {
    assert.equal(fixture.donor.repo, "hive");
    assert.match(fixture.donor.pin, /^e0b8198b/);
    assert.equal(fixture.donor.path, donorPath);
  }
  assert.equal(BUNDLE.bundle_constants.version, 1);
  assert.equal(BUNDLE.bundle_constants.sharePath, "/workspace");
});

// ── Bundle law ──────────────────────────────────────────────────────────────

test("attachment mode matrix re-derives from the mode, the source state, and the naming law", () => {
  for (const vector of BUNDLE.attachment_cases) {
    const derived = applyAttachmentMode(vector.attachment, vector.mode, vector.attachment.sourceExists);
    assert.deepEqual(
      { ...derived.fields, bundled: derived.bundled },
      vector.expected,
      vector.name,
    );
  }
});

test("share-path rewrite traverses strings, arrays, and objects — and an empty workspace path rewrites nothing", () => {
  for (const vector of BUNDLE.path_rewrite_cases) {
    const derived = rewriteShare(vector.input, vector.workspacePath, BUNDLE.bundle_constants.sharePath);
    assert.deepEqual(derived, vector.expected, vector.name);
  }
});

test("a rewritten bundle never contains the workspace path and pins localPath to the placeholder", () => {
  const workspacePath = "/Users/op/secret project";
  const bundle = rewriteShare(
    {
      version: BUNDLE.bundle_constants.version,
      chatId: "chat-1",
      title: "Review",
      localPath: workspacePath,
      messages: [
        { kind: "assistant_text", text: `Edited ${workspacePath}/src/main.rs` },
        { kind: "user_prompt", content: "see attachment", attachments: [{ id: "a", displayName: "x.png" }] },
      ],
    },
    workspacePath,
    BUNDLE.bundle_constants.sharePath,
  );
  assert.equal(bundle.localPath, "/workspace");
  assert.ok(!JSON.stringify(bundle).includes(workspacePath));
});

// ── Naming law ──────────────────────────────────────────────────────────────

test("segment sanitization vectors re-derive", () => {
  for (const vector of NAMING.sanitize_cases) {
    assert.equal(sanitizeSegment(vector.input), vector.expected, JSON.stringify(vector.input));
  }
});

test("export timestamp vectors re-derive", () => {
  for (const vector of NAMING.timestamp_cases) {
    assert.equal(exportTimestamp(vector.input), vector.expected, vector.input);
  }
});

test("unique export directory vectors re-derive, including fallbacks and collision suffixes", () => {
  for (const vector of NAMING.directory_cases) {
    assert.equal(
      uniqueExportDir(vector.title, vector.chatId, vector.timestamp, vector.taken),
      vector.expected,
      vector.name,
    );
  }
});

// ── Release-manifest law ────────────────────────────────────────────────────

test("release asset names flatten slashes to double underscores", () => {
  for (const vector of MANIFEST.asset_name_cases) {
    assert.equal(assetName(vector.input), vector.expected, vector.input);
  }
});

test("cache control splits html from immutable assets", () => {
  for (const vector of MANIFEST.cache_control_cases) {
    assert.equal(cacheControl(vector.input), vector.expected, vector.input);
  }
});

test("content types resolve case-insensitively from the closed table with an octet-stream fallback", () => {
  for (const vector of MANIFEST.content_type_cases) {
    assert.equal(contentType(vector.input), vector.expected, vector.input);
  }
});

test("release tags never double the leading v", () => {
  for (const vector of MANIFEST.release_tag_cases) {
    assert.equal(releaseTag(vector.input), vector.expected, vector.input);
  }
});

test("the manifest shape is frozen", () => {
  assert.deepEqual(MANIFEST.manifest_shape.top_level_keys, ["viewerVersion", "releaseTag", "generatedAt", "files"]);
  assert.deepEqual(MANIFEST.manifest_shape.per_file_keys, ["assetName", "cacheControl", "contentType"]);
  assert.equal(MANIFEST.manifest_shape.manifest_asset_name, "export-viewer-manifest.json");
});
