import { expect, type Locator, type Page } from "@playwright/test";

import { waitForAnimations } from "./animations";

/**
 * Pick an item from a menu that may still be settling, and prove it landed.
 *
 * Radix menus animate in, and Playwright's stability check races that
 * animation. The click is held while the item's box keeps moving ("element is
 * not stable", usually twice), then force-retried — and the retry's own
 * interaction detaches the content, after which the action waits out the
 * whole test timeout instead of failing fast. A trigger click landing too
 * early is the mirror image: the menu never opens, and the item the test is
 * waiting for never appears at all.
 *
 * Waiting for animations alone does not close either one: measured on the
 * video review speed menu at 3 failures in 10 runs with that wait in place.
 * What works is to bound each attempt so one cannot swallow the test, reopen
 * the menu when an attempt lost it, and assert the outcome the caller
 * actually needs rather than trusting the click.
 */
export async function clickSettlingMenuItem(input: {
  page: Page;
  /** Reopens the menu when an attempt closed it. */
  trigger: Locator;
  item: Locator;
  /** What proves the pick landed. Runs inside the retry. */
  verify: () => Promise<void>;
  timeout?: number;
}) {
  await expect(async () => {
    if ((await input.item.count()) === 0) await input.trigger.click();
    await waitForAnimations(input.page);
    await input.item.click({ timeout: 5_000 });
    await input.verify();
  }).toPass({ timeout: input.timeout ?? 20_000 });
}
