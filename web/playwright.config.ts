import { defineConfig, devices } from "@playwright/test";

// Parallel worktrees each run their own preview server; E2E_PORT (or the
// older BUZZ_E2E_PORT) keeps them apart, in the same order the desktop config
// reads them (desktop/tests/helpers/previewOrigin.ts).
const PORT = Number(process.env.E2E_PORT || process.env.BUZZ_E2E_PORT || 4173);
const BASE_URL = `http://127.0.0.1:${PORT}`;

export default defineConfig({
  testDir: "./tests/e2e",
  timeout: 30_000,
  retries: process.env.CI ? 2 : 0,
  workers: 1,
  reporter: [
    ["list"],
    ["html", { open: "never", outputFolder: "playwright-report" }],
  ],
  use: {
    baseURL: BASE_URL,
    screenshot: "only-on-failure",
    trace: "on-first-retry",
    video: "retain-on-failure",
  },
  projects: [
    {
      name: "smoke",
      testMatch: [
        "**/smoke.spec.ts",
        "**/sessions.spec.ts",
        "**/coding-session-prose-join.spec.ts",
        "**/coding-session-auto-title.spec.ts",
      ],
      use: {
        ...devices["Desktop Chrome"],
      },
    },
  ],
  webServer: {
    command: `pnpm exec vite preview --port ${PORT} --strictPort --host 127.0.0.1`,
    cwd: ".",
    reuseExistingServer: !process.env.CI,
    url: BASE_URL,
  },
});
