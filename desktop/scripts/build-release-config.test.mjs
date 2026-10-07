import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const SCRIPT = fileURLToPath(
  new URL("./build-release-config.mjs", import.meta.url),
);

/** Runs the script in a scratch cwd with only `env` set for the updater. */
function run(env) {
  const cwd = mkdtempSync(join(tmpdir(), "release-config-"));
  mkdirSync(join(cwd, "src-tauri"));
  const base = { ...process.env };
  for (const key of Object.keys(base)) {
    if (/^(BEEKEEPER|BUZZ)_UPDATER_/.test(key)) delete base[key];
  }
  try {
    const result = spawnSync(process.execPath, [SCRIPT], {
      cwd,
      env: { ...base, ...env },
      encoding: "utf8",
    });
    const config =
      result.status === 0
        ? JSON.parse(
            readFileSync(
              join(cwd, "src-tauri/tauri.release.conf.json"),
              "utf8",
            ),
          )
        : null;
    return { status: result.status, stderr: result.stderr, config };
  } finally {
    rmSync(cwd, { recursive: true, force: true });
  }
}

test("updater config reads the pre-rename BUZZ_UPDATER_* names", () => {
  const { status, config } = run({
    BUZZ_UPDATER_PUBLIC_KEY: "legacy-pubkey",
    BUZZ_UPDATER_ENDPOINT: "https://legacy.example/update",
  });
  assert.equal(status, 0);
  assert.equal(config.plugins.updater.pubkey, "legacy-pubkey");
  assert.deepEqual(config.plugins.updater.endpoints, [
    "https://legacy.example/update",
  ]);
});

test("updater config prefers BEEKEEPER_UPDATER_* over the legacy names", () => {
  const { status, config } = run({
    BEEKEEPER_UPDATER_PUBLIC_KEY: "new-pubkey",
    BEEKEEPER_UPDATER_ENDPOINT: "https://new.example/update",
    BUZZ_UPDATER_PUBLIC_KEY: "legacy-pubkey",
    BUZZ_UPDATER_ENDPOINT: "https://legacy.example/update",
  });
  assert.equal(status, 0);
  assert.equal(config.plugins.updater.pubkey, "new-pubkey");
  assert.deepEqual(config.plugins.updater.endpoints, [
    "https://new.example/update",
  ]);
});

test("updater config still fails, naming the new variables, when neither is set", () => {
  const { status, stderr } = run({});
  assert.equal(status, 1);
  assert.match(stderr, /BEEKEEPER_UPDATER_PUBLIC_KEY/);
  assert.match(stderr, /BEEKEEPER_UPDATER_ENDPOINT/);
});
