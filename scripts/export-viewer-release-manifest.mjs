#!/usr/bin/env node
/**
 * H-08 — prepare export-viewer release assets + manifest, LOCALLY ONLY.
 *
 * The law is banked in
 * `conformance/transcript-export/fixtures/release-manifest-vectors.json`
 * (donor: Hive `scripts/prepare-export-viewer-release-assets.ts` at pin
 * `e0b8198bd144`, which shipped with no donor test — the corpus is its
 * first gate, and this script is its Buzz implementation, re-derived, never
 * copied). Given a built viewer dist, it flattens every file into
 * `export-viewer__…` release assets beside `export-viewer-manifest.json`.
 *
 * AUTHORITY BOUNDARY: this script produces local artifacts and nothing
 * else — no upload, no credential, no release authority. Publishing the
 * prepared assets is ledger row H-05c's plane (tier 2, untouched).
 *
 * CLI: node scripts/export-viewer-release-manifest.mjs \
 *        --dist <viewer-dist-dir> --version <version> --out <staging-dir>
 */
import { cpSync, mkdirSync, readdirSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

export function toReleaseAssetName(relativePath) {
  return `export-viewer__${relativePath.split("/").join("__")}`;
}

export function cacheControlFor(relativePath) {
  return relativePath.endsWith(".html")
    ? "public, max-age=300"
    : "public, max-age=31536000, immutable";
}

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

export function contentTypeFor(relativePath) {
  const extension = path.posix.extname(relativePath).toLowerCase();
  return CONTENT_TYPES_BY_EXTENSION[extension] ?? "application/octet-stream";
}

export function deriveReleaseTag(version) {
  return `v${version.replace(/^v/u, "")}`;
}

export function buildManifest(version, relativePaths, generatedAt) {
  const files = {};
  for (const relativePath of relativePaths) {
    files[relativePath] = {
      assetName: toReleaseAssetName(relativePath),
      cacheControl: cacheControlFor(relativePath),
      contentType: contentTypeFor(relativePath),
    };
  }
  return {
    viewerVersion: version.replace(/^v/u, ""),
    releaseTag: deriveReleaseTag(version),
    generatedAt,
    files,
  };
}

function walkDist(distDir) {
  const relativePaths = [];
  const walk = (dir, prefix) => {
    for (const name of readdirSync(dir).sort()) {
      const absolute = path.join(dir, name);
      const relative = prefix ? `${prefix}/${name}` : name;
      if (statSync(absolute).isDirectory()) {
        walk(absolute, relative);
      } else {
        relativePaths.push(relative);
      }
    }
  };
  walk(distDir, "");
  return relativePaths;
}

/**
 * Flatten `distDir` into `outDir` as release assets plus the manifest.
 * Returns the manifest object. Local filesystem only.
 */
export function prepareReleaseAssets({ distDir, version, outDir, generatedAt }) {
  const relativePaths = walkDist(distDir);
  const manifest = buildManifest(version, relativePaths, generatedAt);
  mkdirSync(outDir, { recursive: true });
  for (const relativePath of relativePaths) {
    cpSync(
      path.join(distDir, ...relativePath.split("/")),
      path.join(outDir, manifest.files[relativePath].assetName),
    );
  }
  writeFileSync(
    path.join(outDir, "export-viewer-manifest.json"),
    `${JSON.stringify(manifest, null, 2)}\n`,
  );
  return manifest;
}

function parseArgs(argv) {
  const args = {};
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (!flag?.startsWith("--") || value === undefined) {
      throw new Error(`Malformed argument pair at ${flag ?? "<end>"}`);
    }
    args[flag.slice(2)] = value;
  }
  for (const required of ["dist", "version", "out"]) {
    if (!args[required]) throw new Error(`Missing --${required}`);
  }
  return args;
}

const isMain =
  process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1]);
if (isMain) {
  const args = parseArgs(process.argv.slice(2));
  const manifest = prepareReleaseAssets({
    distDir: args.dist,
    version: args.version,
    outDir: args.out,
    generatedAt: new Date().toISOString(),
  });
  const count = Object.keys(manifest.files).length;
  console.log(
    `Staged ${count} release asset(s) + export-viewer-manifest.json in ${args.out} (tag ${manifest.releaseTag}). Local artifacts only — publication is H-05c's plane.`,
  );
}
