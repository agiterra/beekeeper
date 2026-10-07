import type { TestInfo } from "@playwright/test";

/**
 * Whether `testInfo` belongs to a project driving WebKit — the `smoke-webkit`
 * project (`devices["Desktop Safari"]`). This is the WebKit engine Playwright
 * ships, a proxy for the macOS WKWebView the app runs in, not WKWebView
 * itself.
 */
export function isWebKitProject(testInfo: TestInfo): boolean {
  const use = testInfo.project.use;
  return (use.browserName ?? use.defaultBrowserType) === "webkit";
}

/**
 * Where a spec writes the screenshot `name` under `dir`.
 *
 * Under Chromium the path is `<dir>/<name>.png`, exactly what the specs wrote
 * before this helper existed. Under WebKit it is `<dir>-webkit/<name>.png`, so
 * a run of both projects keeps two sets side by side instead of the second
 * engine silently overwriting the first engine's pixels.
 */
export function shotPath(
  testInfo: TestInfo,
  dir: string,
  name: string,
): string {
  const base = isWebKitProject(testInfo) ? `${dir}-webkit` : dir;
  return `${base}/${name}.png`;
}
