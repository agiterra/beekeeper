import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

/**
 * SV-50 honesty: when the channel member read fails or times out, membership
 * is unknown and every mention is asked about. The prompt must then say it
 * could not confirm membership, never that these people are not in the
 * channel.
 */

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    Node: dom.window.Node,
    MutationObserver: dom.window.MutationObserver,
    CustomEvent: dom.window.CustomEvent,
    Event: dom.window.Event,
    KeyboardEvent: dom.window.KeyboardEvent,
    MouseEvent: dom.window.MouseEvent,
    PointerEvent: dom.window.PointerEvent ?? dom.window.MouseEvent,
    getComputedStyle: dom.window.getComputedStyle,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
  dom.window.matchMedia = () => ({
    matches: false,
    addEventListener() {},
    removeEventListener() {},
  });
});

afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
});

after(() => dom.window.close());

async function renderDialog(membershipUnknown) {
  const { createElement } = await import("react");
  const { render } = await import("@testing-library/react");
  const { NonMemberMentionDialog } = await import(
    "@/features/messages/ui/NonMemberMentionDialog"
  );
  render(
    createElement(NonMemberMentionDialog, {
      canInvite: true,
      error: null,
      isInvitePending: false,
      membershipUnknown,
      names: ["Alice", "Bob"],
      onDismiss() {},
      onDoNothing() {},
      onInvite() {},
      open: true,
    }),
  );
  return dom.window.document.body.textContent ?? "";
}

test("unknown membership: the prompt says it could not confirm, not 'not in this channel'", async () => {
  const text = await renderDialog(true);
  assert.match(
    text,
    /Could not confirm whether Alice, Bob are in this channel\./,
  );
  assert.doesNotMatch(text, /are not in this channel/);
});

test("known membership: the prompt still names who is outside the channel", async () => {
  const text = await renderDialog(false);
  assert.match(text, /Alice, Bob are not in this channel\./);
  assert.doesNotMatch(text, /Could not confirm/);
});

test("the send flow passes the unknown-membership state to the prompt", () => {
  const source = readFileSync(
    new URL("./useMentionSendFlow.ts", import.meta.url),
    "utf8",
  );
  assert.match(
    source,
    /setIsMembershipUnknown\(membership\?\.kind === "unknown"\);\s*setPendingNonMemberSend\(pendingDraft\);/,
  );
  assert.match(source, /membershipUnknown: isMembershipUnknown,/);
  // Every place that closes the prompt also clears the flag.
  const closes = source.match(/setPendingNonMemberSend\(null\);/g) ?? [];
  const clears = source.match(/setIsMembershipUnknown\(false\);/g) ?? [];
  assert.equal(clears.length, closes.length);
});
