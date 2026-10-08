import type {
  SessionPreviewDriving,
  SessionPreviewTarget,
  SessionPreviewUnavailableCode,
} from "@/shared/api/tauriSessionPreview";

/**
 * The Browser surface's words and its pure decisions (SV-33 S1/S2, WIRE-C4
 * §§ 3–4). Every sentence a person reads about the preview is here, word for
 * word, so the reasons test pins one place.
 */

/** WIRE-C4 § 4: the unavailable sentence for each reason code. */
export const SESSION_PREVIEW_UNAVAILABLE_SENTENCES: Readonly<
  Record<SessionPreviewUnavailableCode, string>
> = Object.freeze({
  not_macos: "The Browser runs on macOS in this version.",
  no_session: "Open a session to use the Browser.",
  content_filter_failed:
    "The Browser could not start its local-only filter, so it stays off.",
  webview_failed: "The Browser could not start a web view on this computer.",
});

/** The one separator the strip's two phrases use (spec: a middle dot). */
export const SESSION_PREVIEW_STRIP_SEPARATOR = " · ";

/** Always shown: the preview stays on this computer until C5. */
export const SESSION_PREVIEW_LOCAL_ONLY_LABEL = `Local only${SESSION_PREVIEW_STRIP_SEPARATOR}not shared yet`;

/** The umbrella view cannot name one session: the person chooses. */
export const SESSION_PREVIEW_CHOOSE_DRIVER =
  "Choose which agent may drive this preview";

/** Disclosed when the session has no execution to bind to. */
export const SESSION_PREVIEW_UNBOUND_LABEL =
  "No agent in this session can drive it yet.";

/** The person's own close; matches the broker's `preview_closed_by_person`. */
export const SESSION_PREVIEW_CLOSED_LABEL =
  "You closed this preview. Agents are told not to reopen it unless you ask.";

/** The webview area while it is popped out. */
export const SESSION_PREVIEW_POPPED_LABEL = "Open in its own window.";

/** The slot while the mini-player floats over the transcript. */
export const SESSION_PREVIEW_FLOATING_LABEL = "Floating over the transcript.";

/** WIRE-C4 § 3: the driving strip, name resolved from the session's executions. */
export function sessionPreviewDrivingText(name: string): string {
  return `Agent (${name}) is driving${SESSION_PREVIEW_STRIP_SEPARATOR}synthetic input`;
}

export type SessionPreviewAvailability =
  | { available: true }
  | { available: false; code: SessionPreviewUnavailableCode; reason: string };

/**
 * Whether the Browser can open in this view. Pure: the platform is passed in
 * (`isMacPlatform()` at the call site), so the reasons test pins both.
 * Whether the native view actually starts is Rust's answer (`unavailable` in
 * the state), shown in the panel, not guessed here.
 */
export function sessionPreviewAvailability(input: {
  channelId: string | null | undefined;
  isMac: boolean;
}): SessionPreviewAvailability {
  if (!input.channelId) {
    return {
      available: false,
      code: "no_session",
      reason: SESSION_PREVIEW_UNAVAILABLE_SENTENCES.no_session,
    };
  }
  if (!input.isMac) {
    return {
      available: false,
      code: "not_macos",
      reason: SESSION_PREVIEW_UNAVAILABLE_SENTENCES.not_macos,
    };
  }
  return { available: true };
}

/** One execution the preview could be bound to. */
export type SessionPreviewBindingOption = {
  executionKey: string;
  label: string;
  target: SessionPreviewTarget;
};

/**
 * Which session a person-opened preview is bound to (WIRE-C4 contract change,
 * 2026-10-07): only that session's grants may drive it.
 *
 * - `bound`: the focused execution, the person's choice, or the only one.
 * - `choose`: several executions and none focused or chosen. The surface asks
 *   "Choose which agent may drive this preview" and opens nothing until then.
 * - `none`: no execution names a target. The preview opens bound to no
 *   session (`target: null`) and says so; it is never silently shared.
 */
export type SessionPreviewBinding =
  | { kind: "bound"; option: SessionPreviewBindingOption }
  | { kind: "choose"; options: SessionPreviewBindingOption[] }
  | { kind: "none" };

export function sessionPreviewBinding(input: {
  options: readonly SessionPreviewBindingOption[];
  focusedExecutionKey: string | null;
  chosenExecutionKey: string | null;
}): SessionPreviewBinding {
  const pick = (key: string | null) =>
    key === null
      ? undefined
      : input.options.find((option) => option.executionKey === key);
  const chosen =
    pick(input.chosenExecutionKey) ?? pick(input.focusedExecutionKey);
  if (chosen) return { kind: "bound", option: chosen };
  if (input.options.length === 1) {
    return { kind: "bound", option: input.options[0] };
  }
  if (input.options.length === 0) return { kind: "none" };
  return { kind: "choose", options: [...input.options] };
}

/**
 * The name the driving strip shows: the execution whose target session id
 * the driving op names, else the first 8 characters of that session id.
 * Never a pubkey.
 */
export function sessionPreviewDriverName(
  driving: Pick<SessionPreviewDriving, "sessionId" | "executionId">,
  options: readonly SessionPreviewBindingOption[],
): string {
  const match = options.find(
    (option) =>
      option.target.sessionId === driving.sessionId ||
      option.executionKey === driving.executionId,
  );
  if (match) return match.label;
  return `session ${driving.sessionId.slice(0, 8)}`;
}

/**
 * The URL a toolbar entry or a server row resolves to: bare ports become
 * `http://localhost:<port>/`, a scheme-less host gets `http://`. Policy (what
 * may load) is Rust's; this only spells what the person typed.
 */
export function sessionPreviewNormalizeUrl(raw: string): string {
  const text = raw.trim();
  if (/^\d{1,5}$/.test(text)) return `http://localhost:${text}/`;
  if (/^:\d{1,5}(\/.*)?$/.test(text)) return `http://localhost${text}`;
  if (text === "about:blank") return text;
  // `localhost:5173` reads as scheme `localhost:` to a URL parser.
  if (/^[a-z0-9.-]+:\d{1,5}(\/.*)?$/i.test(text)) return `http://${text}`;
  if (!/^[a-z][a-z0-9+.-]*:/i.test(text)) return `http://${text}`;
  return text;
}

/** Strip the scheme for display in the URL field and recents. */
export function sessionPreviewDisplayUrl(url: string): string {
  return url.replace(/^https?:\/\//, "").replace(/\/$/, "");
}
