import { emit } from "@tauri-apps/api/event";

import type {
  SessionPreviewRect,
  SessionPreviewServer,
  SessionPreviewState,
  SessionPreviewTarget,
} from "@/shared/api/tauriSessionPreview";

import type {
  WaveBMockCommandConfig,
  WaveBMockCommandResult,
} from "./e2eBridgeWaveBRegistry";

/**
 * Mock `session_preview_*` commands (SV-33 S1/S2, lane L2).
 *
 * Inert unless a spec declares `window.__BEEKEEPER_E2E_SESSION_PREVIEW__`
 * before the app loads. **There is no web view here.** `set_rect` draws a
 * fixed box labelled "native webview (mock)" at the slot's rect, inset 2 px
 * and above the whole DOM, the way Rust places the WKWebView; `set_occluded`
 * hides it and hands back a labelled freeze frame. A screenshot of this mock
 * proves the layout and the honesty text only. It cannot prove native
 * stacking, focus or input: those need the real app (BRIEF § 7).
 *
 * The declared object is live state a spec may change mid-test:
 * - `servers`: what `session_preview_servers` lists.
 * - `unavailable`: answer every status with this reason instead.
 * - `refuseExternal`: refuse non-loopback URLs (default true), the broker's
 *   `preview_url_refused` sentence word for word.
 * Calls are recorded on the same object (`navigations`, `rects`, `occluded`)
 * for assertions.
 */
export type SessionPreviewMockState = {
  servers: SessionPreviewServer[];
  unavailable?: { code: string; sentence: string } | null;
  refuseExternal?: boolean;
  previews?: Record<string, SessionPreviewState>;
  navigations?: Array<{ url: string; target: SessionPreviewTarget | null }>;
  rects?: Array<{
    rect: SessionPreviewRect | null;
    windowLabel: string;
    seq: number;
  }>;
  occluded?: boolean[];
};

declare global {
  interface Window {
    __BEEKEEPER_E2E_SESSION_PREVIEW__?: SessionPreviewMockState;
    /** Spec hook: emit agent driving on a channel's mock preview. */
    __BEEKEEPER_E2E_SESSION_PREVIEW_DRIVE__?: (input: {
      channelId: string;
      sessionId: string;
      executionId: string;
      op: string;
    }) => Promise<void>;
  }
}

const OVERLAY_ID_PREFIX = "e2e-mock-native-webview-";
const URL_REFUSED =
  "The Browser only opens pages on this computer (localhost, 127.0.0.1, [::1]).";
const FREEZE_FRAME = `data:image/svg+xml;utf8,${encodeURIComponent(
  '<svg xmlns="http://www.w3.org/2000/svg" width="800" height="500"><rect width="100%" height="100%" fill="#e8e8ef"/><text x="50%" y="50%" text-anchor="middle" font-family="sans-serif" font-size="22" fill="#555">freeze frame (mock): native webview hidden</text></svg>',
)}`;

function mockState(): SessionPreviewMockState | null {
  if (typeof window === "undefined") return null;
  const value = window.__BEEKEEPER_E2E_SESSION_PREVIEW__;
  if (!value) return null;
  value.previews ??= {};
  value.navigations ??= [];
  value.rects ??= [];
  value.occluded ??= [];
  return value;
}

function absent(channelId: string): SessionPreviewState {
  return {
    channelId,
    status: "absent",
    url: null,
    title: null,
    generation: 0,
    canGoBack: false,
    canGoForward: false,
    placement: "none",
    hidden: false,
    occluded: false,
    freezeFrame: null,
    boundTo: { kind: "none" },
    driving: null,
    dataStore: "per_session",
    unavailable: null,
  };
}

function isLoopback(url: string): boolean {
  if (url === "about:blank") return true;
  try {
    const parsed = new URL(url);
    return (
      (parsed.protocol === "http:" || parsed.protocol === "https:") &&
      ["localhost", "127.0.0.1", "[::1]"].includes(parsed.hostname) &&
      parsed.port !== "1420"
    );
  } catch {
    return false;
  }
}

/** Draw (or remove) the labelled stand-in for the native view. */
function paintStandIn(
  state: SessionPreviewState,
  rect: SessionPreviewRect | null,
) {
  const id = `${OVERLAY_ID_PREFIX}${state.channelId}`;
  let box = document.getElementById(id);
  const visible =
    rect !== null &&
    state.placement === "docked" &&
    !state.hidden &&
    (state.status === "ready" || state.status === "loading");
  if (!visible) {
    if (box) box.style.display = "none";
    return;
  }
  if (!box) {
    box = document.createElement("div");
    box.id = id;
    box.setAttribute("data-testid", "e2e-mock-native-webview");
    document.documentElement.appendChild(box);
  }
  Object.assign(box.style, {
    position: "fixed",
    left: `${rect.x + 2}px`,
    top: `${rect.y + 2}px`,
    width: `${Math.max(0, rect.width - 4)}px`,
    height: `${Math.max(0, rect.height - 4)}px`,
    zIndex: "2147483000",
    display: "flex",
    flexDirection: "column",
    alignItems: "center",
    justifyContent: "center",
    gap: "6px",
    background:
      "repeating-linear-gradient(45deg, #f4f4f8, #f4f4f8 12px, #ececf3 12px, #ececf3 24px)",
    border: "1px dashed #8a8aa0",
    font: "12px/1.4 sans-serif",
    color: "#444",
    pointerEvents: "none",
  });
  box.textContent = "";
  const label = document.createElement("strong");
  label.textContent = "native webview (mock)";
  const page = document.createElement("span");
  page.textContent = `${state.title ?? ""} ${state.url ?? ""}`.trim();
  box.append(label, page);
}

const lastRects = new Map<string, SessionPreviewRect | null>();

async function publish(
  mock: SessionPreviewMockState,
  next: SessionPreviewState,
): Promise<SessionPreviewState> {
  (mock.previews as Record<string, SessionPreviewState>)[next.channelId] = next;
  paintStandIn(next, lastRects.get(next.channelId) ?? null);
  await emit("session-preview://state", structuredClone(next));
  return next;
}

function refused(code: string, message: string): never {
  throw { code, message };
}

function titleFor(mock: SessionPreviewMockState, url: string): string | null {
  return mock.servers.find((server) => server.url === url)?.title ?? null;
}

export async function handleSessionPreviewMockCommand(
  command: string,
  payload: unknown,
  _config: WaveBMockCommandConfig,
): Promise<WaveBMockCommandResult> {
  if (!command.startsWith("session_preview_")) return null;
  const mock = mockState();
  // Community switches close every preview, in every spec.
  if (command === "session_preview_close_all") {
    if (mock?.previews) {
      for (const id of Object.keys(mock.previews)) {
        mock.previews[id] = absent(id);
        paintStandIn(mock.previews[id], null);
      }
    }
    return { handled: true, value: { closed: 0 } };
  }
  if (!mock) return null;
  installDriveHook(mock);
  const args = (payload ?? {}) as Record<string, unknown>;
  const channelId = String(args.channelId ?? "");
  const previews = mock.previews as Record<string, SessionPreviewState>;
  const current = previews[channelId] ?? absent(channelId);
  const handled = (value: unknown): WaveBMockCommandResult => ({
    handled: true,
    value,
  });

  if (mock.unavailable && command !== "session_preview_servers") {
    return handled({
      ...current,
      status: "unavailable",
      unavailable: mock.unavailable,
    });
  }

  switch (command) {
    case "session_preview_status":
      return handled(current);
    case "session_preview_servers":
      return handled({ servers: mock.servers });
    case "session_preview_navigate": {
      const url = String(args.url ?? "");
      const target = (args.target ?? null) as SessionPreviewTarget | null;
      mock.navigations?.push({ url, target });
      if ((mock.refuseExternal ?? true) && !isLoopback(url)) {
        refused("preview_url_refused", URL_REFUSED);
      }
      const opened = current.status === "ready" || current.status === "loading";
      return handled(
        await publish(mock, {
          ...current,
          status: "ready",
          url,
          title: titleFor(mock, url),
          generation: current.generation + 1,
          canGoBack: opened,
          canGoForward: false,
          placement:
            current.placement === "popped_out" ? "popped_out" : "docked",
          boundTo:
            current.boundTo && current.boundTo.kind !== "none"
              ? current.boundTo
              : // A person's navigate binds the surface's session (WIRE-C4
                // § 9.8); with no session the preview is undriveable.
                target
                ? {
                    kind: "person",
                    sessionId: target.sessionId,
                    generation: target.generation,
                  }
                : { kind: "none" },
        }),
      );
    }
    case "session_preview_reload":
      return handled(await publish(mock, { ...current }));
    case "session_preview_back":
      return handled(
        await publish(mock, {
          ...current,
          canGoBack: false,
          canGoForward: true,
        }),
      );
    case "session_preview_forward":
      return handled(
        await publish(mock, {
          ...current,
          canGoBack: true,
          canGoForward: false,
        }),
      );
    case "session_preview_close":
      return handled(
        await publish(mock, {
          ...absent(channelId),
          status: "closed_by_person",
        }),
      );
    case "session_preview_popout":
      return handled(
        await publish(mock, {
          ...current,
          placement: args.popped ? "popped_out" : "docked",
        }),
      );
    case "session_preview_set_rect": {
      const rect = (args.rect ?? null) as SessionPreviewRect | null;
      mock.rects?.push({
        rect,
        windowLabel: String(args.windowLabel ?? ""),
        seq: Number(args.seq ?? 0),
      });
      lastRects.set(channelId, rect);
      paintStandIn(current, rect);
      return handled(current);
    }
    case "session_preview_set_occluded": {
      const occluded = Boolean(args.occluded);
      mock.occluded?.push(occluded);
      return handled(
        await publish(mock, {
          ...current,
          occluded,
          hidden: occluded,
          freezeFrame: occluded ? FREEZE_FRAME : null,
        }),
      );
    }
    default:
      return handled(current);
  }
}

function installDriveHook(mock: SessionPreviewMockState) {
  if (window.__BEEKEEPER_E2E_SESSION_PREVIEW_DRIVE__) return;
  window.__BEEKEEPER_E2E_SESSION_PREVIEW_DRIVE__ = async (input) => {
    const previews = mock.previews as Record<string, SessionPreviewState>;
    const current = previews[input.channelId] ?? absent(input.channelId);
    const at = Math.floor(Date.now() / 1000);
    await publish(mock, {
      ...current,
      driving: {
        executionId: input.executionId,
        sessionId: input.sessionId,
        lastOp: input.op,
        at,
      },
    });
    await emit("session-preview://driving", {
      channelId: input.channelId,
      driving: true,
      executionId: input.executionId,
      sessionId: input.sessionId,
      at,
    });
  };
}
