import type {
  SessionPreviewServer,
  SessionPreviewState,
} from "@/shared/api/tauriSessionPreview";

import { sessionPreviewDisplayUrl } from "./previewModel";

/**
 * The Browser tab's lifecycle rules (ledger 371 (c), (d), (i)), pure so the
 * tests pin them: when closing the tab closes the page, what an agent's open
 * does to the person's panels, how a page that is open but drawn nowhere is
 * disclosed, and which server the switcher marks current.
 */

/** The Browser surface's id in the session's panels. */
export const SESSION_PREVIEW_TAB_ID = "browser";

/** What one render of a session view knows about its panels. */
export type SessionPreviewTabSnapshot = {
  channelId: string;
  sessionKey: string;
  lens: string;
  tabs: readonly string[];
  /** The person has acted on these panels (persisted). */
  userActed: boolean;
};

/**
 * Whether the Browser tab went away because the person closed it, so the
 * page should close too. Only a removal from the tab list counts — hiding
 * the panel (`closeRight`), switching tabs, switching sessions, or leaving a
 * lens all keep the page (they only unmount the slot, which hides it). A
 * removal in the same session, the same lens, with `userActed` set, is the
 * person's `close` / `closeMany`. The view's own moves never set
 * `userActed`, and the lens move (`closeForLens`) closes only surfaces the
 * new lens does not list — Browser is listed in every lens — so neither
 * reads as the person closing it. A lens change between the two renders is
 * excluded all the same.
 */
export function sessionPreviewTabClosedByPerson(
  previous: SessionPreviewTabSnapshot | null,
  next: SessionPreviewTabSnapshot,
): boolean {
  if (!previous) return false;
  if (
    previous.channelId !== next.channelId ||
    previous.sessionKey !== next.sessionKey ||
    previous.lens !== next.lens
  ) {
    return false;
  }
  return (
    previous.tabs.includes(SESSION_PREVIEW_TAB_ID) &&
    !next.tabs.includes(SESSION_PREVIEW_TAB_ID) &&
    next.userActed
  );
}

/** A page exists (loading or loaded), wherever it is drawn. */
export function sessionPreviewIsOpen(
  state: Pick<SessionPreviewState, "status"> | null,
): boolean {
  return state?.status === "loading" || state?.status === "ready";
}

/**
 * How a session view answers an agent's open-request (`bee preview open`
 * with no Browser slot mounted): show the Browser tab, never a window.
 * `proactive` respects a person who has not arranged the panels yet;
 * once they have, `open` adds and selects the tab (they already acted, so
 * nothing new is recorded about them); `none` when it is already showing.
 */
export function sessionPreviewOpenRequestMove(panel: {
  rightOpen: boolean;
  active: string | null;
  userActed: boolean;
}): "none" | "proactive" | "open" {
  if (panel.rightOpen && panel.active === SESSION_PREVIEW_TAB_ID) return "none";
  return panel.userActed ? "open" : "proactive";
}

/**
 * The disclosure for a page that is alive but drawn nowhere: `localhost:5000
 * is open, hidden`. `null` when there is nothing hidden to disclose.
 */
export function sessionPreviewHiddenNotice(
  state: Pick<SessionPreviewState, "status" | "placement" | "url"> | null,
): string | null {
  if (!sessionPreviewIsOpen(state) || state?.placement !== "hidden") {
    return null;
  }
  const where = state.url ? sessionPreviewDisplayUrl(state.url) : "A page";
  return `${where} is open, hidden`;
}

/** The server whose origin the page is on, for the switcher's mark. */
export function sessionPreviewCurrentServer(
  servers: readonly SessionPreviewServer[],
  url: string | null,
): SessionPreviewServer | null {
  if (!url) return null;
  const origin = originOf(url);
  if (!origin) return null;
  return servers.find((server) => originOf(server.url) === origin) ?? null;
}

function originOf(url: string): string | null {
  try {
    const parsed = new URL(url);
    const host =
      parsed.hostname === "127.0.0.1" ? "localhost" : parsed.hostname;
    return `${parsed.protocol}//${host}:${parsed.port || (parsed.protocol === "https:" ? "443" : "80")}`;
  } catch {
    return null;
  }
}
