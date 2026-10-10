import * as React from "react";

import {
  listenSessionPreview,
  sessionPreviewClose,
  sessionPreviewStatus,
} from "@/shared/api/tauriSessionPreview";

import {
  SESSION_PREVIEW_TAB_ID,
  type SessionPreviewTabSnapshot,
  sessionPreviewIsOpen,
  sessionPreviewOpenRequestMove,
  sessionPreviewTabClosedByPerson,
} from "../lib/previewTabModel";

/** The panel facts and moves the Browser tab needs from a session view. */
export type SessionPreviewBrowserTabInput = {
  channelId: string;
  sessionKey: string;
  lens: string;
  panelState: {
    rightOpen: boolean;
    tabs: readonly string[];
    active: string | null;
    userActed: boolean;
  };
  panels: {
    open: (id: string) => void;
    openProactive: (ids: readonly string[], activate: string) => void;
  };
};

/**
 * Ties the session's preview to its Browser tab, once per session view:
 *
 * - An agent's open-request (its `bee preview open` found no Browser slot
 *   mounted) opens and selects this view's Browser tab, so the page docks
 *   there instead of in a window of its own (ledger 371(c)). A view that is
 *   not on screen is not mounted and so does nothing: the page stays hidden
 *   and disclosed.
 * - The person closing the Browser tab closes the page as theirs
 *   (`closed_by_person`), rather than leaving it alive and hidden
 *   (ledger 371(d)). Unmounts — another session, another tab, a hidden
 *   panel — only hide it.
 */
export function useSessionPreviewBrowserTab(
  input: SessionPreviewBrowserTabInput,
): void {
  const { channelId, sessionKey, lens, panelState, panels } = input;
  const latest = React.useRef({ panelState, panels });
  latest.current = { panelState, panels };

  React.useEffect(() => {
    if (!channelId) return;
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    void listenSessionPreview(channelId, {
      onOpenRequested: () => {
        const { panelState: panel, panels: actions } = latest.current;
        const move = sessionPreviewOpenRequestMove(panel);
        if (move === "proactive") {
          actions.openProactive(
            [SESSION_PREVIEW_TAB_ID],
            SESSION_PREVIEW_TAB_ID,
          );
        } else if (move === "open") {
          actions.open(SESSION_PREVIEW_TAB_ID);
        }
      },
    })
      .then((stop) => {
        if (cancelled) stop();
        else unlisten = stop;
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [channelId]);

  const previous = React.useRef<SessionPreviewTabSnapshot | null>(null);
  React.useEffect(() => {
    const next: SessionPreviewTabSnapshot = {
      channelId,
      sessionKey,
      lens,
      tabs: panelState.tabs,
      userActed: panelState.userActed,
    };
    const closed = sessionPreviewTabClosedByPerson(previous.current, next);
    previous.current = next;
    if (!closed || !channelId) return;
    // Close only a page that exists: closing an absent one would tell the
    // agent the person closed something it never opened.
    void sessionPreviewStatus(channelId)
      .then((state) =>
        sessionPreviewIsOpen(state) ? sessionPreviewClose(channelId) : null,
      )
      .catch(() => {
        // No native Browser here (a plain browser build): nothing to close.
      });
  }, [channelId, sessionKey, lens, panelState.tabs, panelState.userActed]);
}
