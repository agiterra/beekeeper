/**
 * H-08 — release-manifest preparation tests. The law is banked in
 * `conformance/transcript-export/fixtures/release-manifest-vectors.json`
 * (donor: Hive `scripts/prepare-export-viewer-release-assets.ts` at pin
 * `e0b8198bd144`, which shipped with no donor test — the corpus is its first
 * gate and this script is its Beekeeper implementation). Vectors are restated
 * here; the conformance bridge additionally binds the same functions to the
 * fixture file itself. Local artifacts only — no upload, no release
 * authority (publication is H-05c's plane).
 */
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import {
  buildManifest,
  cacheControlFor,
  contentTypeFor,
  deriveReleaseTag,
  prepareReleaseAssets,
  toReleaseAssetName,
} from "./export-viewer-release-manifest.mjs";

test("asset names flatten slashes to double underscores", () => {
  assert.equal(toReleaseAssetName("index.html"), "export-viewer__index.html");
  assert.equal(
    toReleaseAssetName("assets/viewer.js"),
    "export-viewer__assets__viewer.js",
  );
  assert.equal(
    toReleaseAssetName("assets/fonts/brand.woff2"),
    "export-viewer__assets__fonts__brand.woff2",
  );
});

test("cache control splits html from immutable assets", () => {
  assert.equal(cacheControlFor("index.html"), "public, max-age=300");
  assert.equal(cacheControlFor("nested/page.html"), "public, max-age=300");
  assert.equal(
    cacheControlFor("assets/viewer.js"),
    "public, max-age=31536000, immutable",
  );
  assert.equal(
    cacheControlFor("manifest.webmanifest"),
    "public, max-age=31536000, immutable",
  );
});

test("content types resolve case-insensitively from the closed table with an octet-stream fallback", () => {
  const vectors = [
    ["assets/viewer.js", "text/javascript; charset=utf-8"],
    ["styles/app.css", "text/css; charset=utf-8"],
    ["index.html", "text/html; charset=utf-8"],
    ["icons/app.png", "image/png"],
    ["fonts/brand.woff2", "font/woff2"],
    ["app.webmanifest", "application/manifest+json; charset=utf-8"],
    ["shouty/APP.JS", "text/javascript; charset=utf-8"],
    ["module.wasm", "application/octet-stream"],
    ["LICENSE", "application/octet-stream"],
  ];
  for (const [input, expected] of vectors) {
    assert.equal(contentTypeFor(input), expected, input);
  }
});

test("release tags never double the leading v", () => {
  assert.equal(deriveReleaseTag("1.2.3"), "v1.2.3");
  assert.equal(deriveReleaseTag("v1.2.3"), "v1.2.3");
});

test("the manifest shape is frozen", () => {
  const manifest = buildManifest(
    "1.2.3",
    ["index.html", "assets/viewer.js"],
    "2026-08-06T00:00:00.000Z",
  );
  assert.deepEqual(Object.keys(manifest), [
    "viewerVersion",
    "releaseTag",
    "generatedAt",
    "files",
  ]);
  assert.equal(manifest.viewerVersion, "1.2.3");
  assert.equal(manifest.releaseTag, "v1.2.3");
  assert.equal(manifest.generatedAt, "2026-08-06T00:00:00.000Z");
  assert.deepEqual(manifest.files["assets/viewer.js"], {
    assetName: "export-viewer__assets__viewer.js",
    cacheControl: "public, max-age=31536000, immutable",
    contentType: "text/javascript; charset=utf-8",
  });
});

test("prepareReleaseAssets stages a flattened dist plus the manifest, locally only", () => {
  const scratch = mkdtempSync(path.join(tmpdir(), "export-viewer-manifest-"));
  const dist = path.join(scratch, "dist");
  mkdirSync(path.join(dist, "assets", "fonts"), { recursive: true });
  writeFileSync(path.join(dist, "index.html"), "<!doctype html>");
  writeFileSync(path.join(dist, "assets", "viewer.js"), "js");
  writeFileSync(path.join(dist, "assets", "fonts", "brand.woff2"), "font");
  writeFileSync(path.join(dist, "LICENSE"), "license");
  const out = path.join(scratch, "out");

  const manifest = prepareReleaseAssets({
    distDir: dist,
    version: "v2.0.0",
    outDir: out,
    generatedAt: "2026-08-06T00:00:00.000Z",
  });

  assert.equal(manifest.releaseTag, "v2.0.0");
  assert.deepEqual(Object.keys(manifest.files).sort(), [
    "LICENSE",
    "assets/fonts/brand.woff2",
    "assets/viewer.js",
    "index.html",
  ]);

  const staged = readdirSync(out).sort();
  assert.deepEqual(staged, [
    "export-viewer-manifest.json",
    "export-viewer__LICENSE",
    "export-viewer__assets__fonts__brand.woff2",
    "export-viewer__assets__viewer.js",
    "export-viewer__index.html",
  ]);
  assert.equal(
    readFileSync(path.join(out, "export-viewer__assets__viewer.js"), "utf8"),
    "js",
  );
  const written = JSON.parse(
    readFileSync(path.join(out, "export-viewer-manifest.json"), "utf8"),
  );
  assert.deepEqual(written, manifest);
});
