import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import type { Page } from "@playwright/test";

const ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../../..",
);

/** The built runner and registries (`just plugins-build`). */
export const PLUGIN_RUNNER =
  process.env.BEEKEEPER_E2E_PLUGIN_RUNNER ??
  path.join(ROOT, "target/debug/beekeeper-plugin-run");
export const FIRST_PARTY_REGISTRY = path.join(
  ROOT,
  "target/plugins/registry.toml",
);
export const TEST_REGISTRY = path.join(
  ROOT,
  "target/plugins/registry.test.toml",
);

function run(request: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const child = spawn(PLUGIN_RUNNER, [], { stdio: ["pipe", "pipe", "pipe"] });
    let out = "";
    let err = "";
    child.stdout.on("data", (chunk) => {
      out += chunk;
    });
    child.stderr.on("data", (chunk) => {
      err += chunk;
    });
    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0) resolve(out);
      else reject(new Error(`beekeeper-plugin-run exited ${code}: ${err}`));
    });
    child.stdin.end(request);
  });
}

/**
 * Make the mock relay run plugins: every `relay_plugin_invoke` the app makes
 * in mock mode runs the REAL component through the REAL host
 * (`beekeeper-plugin-run`), over the records the mock store holds. Call
 * before `installMockBridge`. `registry` picks which plugins the mock relay
 * "runs" — the first-party registry by default.
 */
export async function installPluginRunner(
  page: Page,
  registry: string = FIRST_PARTY_REGISTRY,
): Promise<void> {
  for (const file of [PLUGIN_RUNNER, registry]) {
    if (!existsSync(file)) {
      throw new Error(
        `${file} is missing — run \`just plugins-build\` and \`cargo build -p beekeeper-plugin-tools\` first`,
      );
    }
  }
  await page.exposeFunction(
    "__BEEKEEPER_E2E_PLUGIN_RUNNER__",
    async (request: string) => {
      const parsed = JSON.parse(request) as Record<string, unknown>;
      return run(JSON.stringify({ ...parsed, registry }));
    },
  );
}
