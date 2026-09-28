import { expect, test, type Page } from "@playwright/test";

import { installMockBridge } from "../helpers/bridge";

const SESSION = {
  sessionId: "mock-shell-1",
  title: "mock-shell",
  currentDirectory: "/Users/you",
  shell: "/bin/zsh",
  createdAt: 1_784_919_914,
  rows: 24,
  cols: 80,
  running: true,
  restorable: false,
};
// Three prompts from a shell theme that queried the terminal each time: the
// background colour (OSC 11 ?) and the cursor position (CSI 6n).
const SCROLLBACK = "\x1b]11;?\x07\x1b[6n$ ".repeat(3);

function ptyWrites(page: Page): Promise<string> {
  return page.evaluate(() =>
    (window.__BUZZ_E2E_COMMAND_LOG__ ?? [])
      .filter((entry) => entry.command === "write_shell_session")
      .map((entry) => (entry.payload as { data: string }).data)
      .join(""),
  );
}

test("opening a shell tab does not answer its replayed scrollback's queries into the PTY", async ({
  page,
}) => {
  await page.addInitScript(
    ([session, scrollbackB64]) => {
      window.__BUZZ_E2E_SHELL_SESSIONS__ = [session];
      window.__BUZZ_E2E_SHELL_SCROLLBACK_B64__ = scrollbackB64;
    },
    [SESSION, Buffer.from(SCROLLBACK).toString("base64")] as const,
  );
  await installMockBridge(page);
  await page.goto("/");
  await page.evaluate(() => {
    window.location.hash = "#/shell/mock-shell-1";
  });
  const terminal = page.getByTestId("shell-terminal");
  await expect(terminal).toBeVisible();
  // The replay has been parsed once its prompts render.
  await expect(terminal.locator(".xterm-rows")).toContainText("$ $ $");

  // Some answers (the cursor-position report) are emitted a beat after the
  // replay parses; give them the time to arrive before asserting none did.
  await page.waitForTimeout(750);
  // Before the fix this read "\x1b]11;rgb:…\x1b\\\x1b[2;1R" three times over:
  // the fresh terminal answering queries a program sent long ago, delivered
  // to the shell as if typed.
  expect(await ptyWrites(page)).toBe("");

  // Typing still reaches the PTY straight after the replay.
  await terminal.click();
  await page.keyboard.type("ls");
  await expect.poll(() => ptyWrites(page)).toBe("ls");
});
