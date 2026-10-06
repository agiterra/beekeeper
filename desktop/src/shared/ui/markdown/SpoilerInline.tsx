import * as React from "react";

import { hasBlockMedia } from "../markdownUtils";
import { SpoilerParticles } from "../SpoilerParticles";

/**
 * True for descendants of a spoiler that is currently hidden. Consumers (e.g.
 * `MaskedLinkTooltip`) use it to suppress hover/focus affordances that would
 * otherwise leak masked content before the spoiler is revealed. Default
 * `false` — content outside any spoiler is never hidden.
 */
export const SpoilerHiddenContext = React.createContext(false);

/** Controls inside revealed content that own their own clicks. */
const INTERACTIVE_CHILD_SELECTOR =
  'a[href], area[href], audio, button, details, input, label, select, summary, textarea, video, [role="button"], [role="link"], [role="checkbox"], [role="menuitem"]';

/**
 * True when a click inside a revealed spoiler landed on a control of its own.
 *
 * The spoiler itself carries `role="button"`, so the closest match is compared
 * against `currentTarget`: a click on the mask is the mask's, a click on a link
 * it was hiding is the link's.
 */
function clickLandedOnInteractiveChild(
  event: React.MouseEvent<HTMLElement>,
): boolean {
  const { target } = event;
  if (!(target instanceof Element)) return false;
  const control = target.closest(INTERACTIVE_CHILD_SELECTOR);
  return control !== null && control !== event.currentTarget;
}

export function SpoilerInline({
  block = false,
  children,
  interactive = true,
}: {
  block?: boolean;
  children?: React.ReactNode;
  interactive?: boolean;
}) {
  const [revealed, setRevealed] = React.useState(false);
  const contentRef = React.useRef<HTMLElement | null>(null);
  const isBlock = block || hasBlockMedia(React.Children.toArray(children));

  const setContentElement = React.useCallback((node: HTMLElement | null) => {
    contentRef.current = node;
  }, []);

  const toggleRevealed = React.useCallback(() => {
    setRevealed((value) => !value);
  }, []);

  const handlePointerDownCapture = React.useCallback(
    (event: React.PointerEvent<HTMLElement>) => {
      if (revealed) return;
      event.stopPropagation();
    },
    [revealed],
  );

  const handleClickCapture = React.useCallback(
    (event: React.MouseEvent<HTMLElement>) => {
      if (revealed) return;
      event.preventDefault();
      event.stopPropagation();
      toggleRevealed();
    },
    [revealed, toggleRevealed],
  );

  const handleClick = React.useCallback(
    (event: React.MouseEvent<HTMLElement>) => {
      // The capture handler owns every click made while the spoiler is hidden,
      // including the one that reveals it, and it stops propagation so this
      // never runs for the same click. Bailing on the state rather than
      // trusting that is what makes a reveal exactly one toggle: if both
      // handlers ever saw one click, the second would undo the first and the
      // spoiler would snap back to hidden.
      if (!revealed) return;
      if (isBlock && event.target !== event.currentTarget) return;
      // Revealed content is ordinary content: a click on a link or button
      // inside it belongs to that control, not to the mask.
      if (clickLandedOnInteractiveChild(event)) return;
      toggleRevealed();
    },
    [isBlock, revealed, toggleRevealed],
  );

  const handleKeyDown = React.useCallback(
    (event: React.KeyboardEvent<HTMLElement>) => {
      if (event.key !== "Enter" && event.key !== " ") return;
      event.preventDefault();
      toggleRevealed();
    },
    [toggleRevealed],
  );

  const revealProps = {
    "aria-label": revealed ? "Hide spoiler" : "Reveal spoiler",
    "aria-pressed": revealed,
    onClick: handleClick,
    onClickCapture: handleClickCapture,
    onKeyDown: handleKeyDown,
    onPointerDownCapture: handlePointerDownCapture,
    role: "button",
    tabIndex: 0,
  } as const;

  if (!interactive) {
    if (isBlock) {
      return (
        <div
          className="beekeeper-spoiler beekeeper-spoiler--block beekeeper-spoiler--inert"
          data-revealed="false"
          data-spoiler=""
        >
          <SpoilerParticles active contentRef={contentRef} />
          <div className="buzz-spoiler__content" ref={setContentElement}>
            <SpoilerHiddenContext.Provider value={true}>
              {children}
            </SpoilerHiddenContext.Provider>
          </div>
        </div>
      );
    }

    return (
      <span
        className="beekeeper-spoiler beekeeper-spoiler--inert"
        data-revealed="false"
        data-spoiler=""
      >
        <SpoilerParticles active contentRef={contentRef} />
        <span className="buzz-spoiler__content" ref={setContentElement}>
          <SpoilerHiddenContext.Provider value={true}>
            {children}
          </SpoilerHiddenContext.Provider>
        </span>
      </span>
    );
  }

  if (isBlock) {
    return (
      <div
        {...revealProps}
        className="beekeeper-spoiler beekeeper-spoiler--block"
        data-revealed={revealed ? "true" : "false"}
        data-spoiler=""
      >
        <SpoilerParticles active={!revealed} contentRef={contentRef} />
        <div className="buzz-spoiler__content" ref={setContentElement}>
          <SpoilerHiddenContext.Provider value={!revealed}>
            {children}
          </SpoilerHiddenContext.Provider>
        </div>
      </div>
    );
  }

  return (
    <span
      {...revealProps}
      className="beekeeper-spoiler"
      data-revealed={revealed ? "true" : "false"}
      data-spoiler=""
    >
      <SpoilerParticles active={!revealed} contentRef={contentRef} />
      <span className="buzz-spoiler__content" ref={setContentElement}>
        <SpoilerHiddenContext.Provider value={!revealed}>
          {children}
        </SpoilerHiddenContext.Provider>
      </span>
    </span>
  );
}
