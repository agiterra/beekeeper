import { writeFileSync } from "node:fs";
import { resolve } from "node:path";

// Write a tauri.release.conf.json with release-only overrides.
//
// Tauri's --config flag merges the provided JSON on top of the base
// tauri.conf.json, so this file must contain ONLY the delta fields —
// not a copy of the base config.
//
// For OSS release builds this script emits:
// 1. bundle.macOS.minimumSystemVersion = "10.15" for broad compatibility.
// 2. bundle.createUpdaterArtifacts = true so Tauri produces the .tar.gz
//    archive and .sig signature during the build.
// 3. plugins.updater with the public key and endpoint from env vars.
//    Both BEEKEEPER_UPDATER_PUBLIC_KEY and BEEKEEPER_UPDATER_ENDPOINT are required -
//    the script fails if either is missing (OSS builds always ship with updater).
//
// No signingIdentity is emitted here and the Tauri build is invoked with
// --no-sign: Apple code signing and notarization were a post-build step of
// Block's release workflow, which is removed (see RELEASING.md). A signing
// lane must be rebuilt before a signed desktop release can ship.

const outputConfigPath = resolve(
  process.cwd(),
  "src-tauri/tauri.release.conf.json",
);

const updaterPubkey = process.env.BEEKEEPER_UPDATER_PUBLIC_KEY;
const updaterEndpoint = process.env.BEEKEEPER_UPDATER_ENDPOINT;

const missing = [];
if (!updaterPubkey) missing.push("BEEKEEPER_UPDATER_PUBLIC_KEY");
if (!updaterEndpoint) missing.push("BEEKEEPER_UPDATER_ENDPOINT");
if (missing.length > 0) {
  console.error(
    `Error: required environment variable(s) missing: ${missing.join(", ")}`,
  );
  process.exit(1);
}

const releaseConfig = {
  bundle: {
    macOS: {
      minimumSystemVersion: "10.15",
    },
    createUpdaterArtifacts: true,
  },
  plugins: {
    updater: {
      pubkey: updaterPubkey,
      endpoints: [updaterEndpoint],
    },
  },
};

// Tauri applies --config after platform-specific config using RFC 7396.
// Any externalBin value here would therefore replace the platform sidecar list,
// while null would silently delete it. This delta must never own that key.
if (Object.hasOwn(releaseConfig.bundle, "externalBin")) {
  throw new Error(
    "Release config must not define bundle.externalBin; sidecars are platform-specific",
  );
}

// Same rule, same reason, for the nested menu bar app. `bundle.macOS.files` in
// the base config is what nests `Beekeeper Menu Bar.app` at
// `Contents/Library/LoginItems/` during bundling; a `files` key here would
// replace it and `null` would delete it, and the result — a login item that
// silently stops being shipped — looks like a menu bar icon that just stopped
// appearing after an update.
//
// `minimumSystemVersion` above is safe precisely because merge-patch merges
// objects key by key: it adds to `macOS` rather than replacing it.
if (Object.hasOwn(releaseConfig.bundle.macOS, "files")) {
  throw new Error(
    "Release config must not define bundle.macOS.files; the nested login item is declared in the base config",
  );
}

console.log(`Updater enabled -> ${updaterEndpoint}`);

writeFileSync(outputConfigPath, `${JSON.stringify(releaseConfig, null, 2)}\n`);
console.log(`Wrote ${outputConfigPath}`);
