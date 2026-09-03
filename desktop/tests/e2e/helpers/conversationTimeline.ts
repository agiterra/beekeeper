import { expect, type Page } from "@playwright/test";

/**
 * Wait until the conversation timeline is the settled, scrollable pane.
 *
 * `data-testid="message-timeline"` is worn by **two different elements** over a
 * channel's life. Before the message list renders, `MessageTimeline.tsx:741-749`
 * puts it (and `data-buzz-conversation-scroll`) on the empty/intro container
 * declaratively. Once the list renders, that container drops both ids and
 * Virtua's own scroller takes them over imperatively, in a layout effect
 * (`TimelineMessageList.tsx:663-665`).
 *
 * So `expect(getByTestId("message-timeline")).toBeVisible()` is satisfied by the
 * *first* of those — an intro surface with nothing to scroll. A wheel gesture
 * over it is dead space, the viewport lock swallows it, and an assertion that
 * the conversation pane keeps its elastic affordance sees `true` where it
 * expects `false`. Under full-suite load the intro surface simply lingers
 * longer, which is why the failure looked like load rather than identity.
 *
 * This waits for the state the assertion is actually about: one element that
 * claims the timeline id, is marked as a conversation scroller, and has
 * something to scroll.
 */
export async function waitForScrollableConversationTimeline(page: Page) {
  await expect
    .poll(() =>
      page.evaluate(() => {
        const scrollers = document.querySelectorAll<HTMLElement>(
          '[data-testid="message-timeline"]',
        );
        // More than one means the hand-off is mid-flight; neither is settled.
        if (scrollers.length !== 1) return -1;
        const scroller = scrollers[0];
        if (scroller.dataset.buzzConversationScroll !== "true") return -1;
        return scroller.scrollHeight - scroller.clientHeight;
      }),
    )
    .toBeGreaterThan(1);
}
