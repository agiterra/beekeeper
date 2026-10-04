/**
 * The origin the E2E preview server answers on.
 *
 * One definition shared by `playwright.config.ts` and the specs that need the
 * absolute origin (clipboard grants, absolute media URLs), so the port moves
 * every one of them together. Several worktrees run E2E at once, and
 * Playwright reuses whatever server already answers on its port: with one
 * shared default, a worktree that forgot to set a port would quietly test a
 * sibling's `dist` against its own specs.
 *
 * So the port is, in order: `E2E_PORT`; `BUZZ_E2E_PORT` (the older spelling);
 * 4173 under `CI` (one checkout per container); otherwise a port derived from
 * this checkout's path, in 4300–4999 — stable for one worktree (so its own
 * server is reused across runs) and different between worktrees.
 * `node desktop/scripts/e2e-affected.mjs port` prints it.
 */
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/** FNV-1a of `text`, folded into 4300–4999. */
export function portForCheckout(text: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return String(4300 + (h % 700));
}

const checkout = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

export const PREVIEW_PORT =
  process.env.E2E_PORT ||
  process.env.BUZZ_E2E_PORT ||
  (process.env.CI ? "4173" : portForCheckout(checkout));

/** `http://127.0.0.1:<PREVIEW_PORT>`, with no trailing slash. */
export const PREVIEW_ORIGIN = `http://127.0.0.1:${PREVIEW_PORT}`;
