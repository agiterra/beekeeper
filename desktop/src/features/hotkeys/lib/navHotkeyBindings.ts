/**
 * The reassignable navigation hotkeys.
 *
 * Two families, each armed by holding a modifier:
 *
 * - **scope** (default Option) reaches the top-level destinations — Dashboard,
 *   Projects, Direct messages — and the projects themselves by position.
 * - **item** (default Command) reaches the rows inside whatever surface you
 *   are already in: a project's children, or your direct messages.
 *
 * Only the two modifiers, the three named keys, and the on/off switch are
 * reassignable. The digits are fixed: making them bindable would multiply the
 * settings surface by twenty to buy nothing, since position is the whole point
 * of a numbered row.
 *
 * Kept pure (no DOM, no storage) so the matching rules can be unit tested;
 * persistence lives in `navHotkeyStorage.ts` and dispatch in
 * `@/app/navigation/useNavigationHotkeys`.
 */

export type NavHotkeyModifier = "alt" | "meta" | "ctrl";

export type NavHotkeyAction = "dashboard" | "projects" | "dms";

export type NavHotkeyBindings = {
  version: 1;
  enabled: boolean;
  /** Arms the top-level destinations and the numbered project list. */
  scopeModifier: NavHotkeyModifier;
  /** Arms the numbered rows of the surface currently open. */
  itemModifier: NavHotkeyModifier;
  /** `KeyboardEvent.code` per named action. */
  codes: Record<NavHotkeyAction, string>;
};

export const NAV_HOTKEY_ACTIONS: readonly NavHotkeyAction[] = [
  "dashboard",
  "projects",
  "dms",
];

export const NAV_HOTKEY_MODIFIERS: readonly NavHotkeyModifier[] = [
  "alt",
  "meta",
  "ctrl",
];

export const DEFAULT_NAV_HOTKEY_BINDINGS: NavHotkeyBindings = Object.freeze({
  version: 1,
  enabled: true,
  scopeModifier: "alt",
  itemModifier: "meta",
  codes: Object.freeze({
    dashboard: "KeyD",
    projects: "KeyP",
    dms: "KeyM",
  }),
}) as NavHotkeyBindings;

/**
 * Positions reachable by a digit: 1 through 9, and no further.
 *
 * `0` is deliberately not the tenth. On macOS ⌘0 already resets the zoom
 * (`useWebviewZoomShortcuts`), and the item modifier defaults to ⌘ — claiming
 * it would silently break a control people rely on to get their text back to
 * a readable size, in exchange for one more row. A row past the ninth gets no
 * badge and no chord rather than a badge reading "10" over a chord no
 * keyboard can send.
 */
export const NAV_HOTKEY_DIGIT_CODES: readonly string[] = [
  "Digit1",
  "Digit2",
  "Digit3",
  "Digit4",
  "Digit5",
  "Digit6",
  "Digit7",
  "Digit8",
  "Digit9",
];

export const NAV_HOTKEY_MAX_POSITIONS = NAV_HOTKEY_DIGIT_CODES.length;

/** The 1-based badge number for a zero-based row position, or null past the cap. */
export function navHotkeyDigitLabel(index: number): string | null {
  if (index < 0 || index >= NAV_HOTKEY_MAX_POSITIONS) return null;
  return String(index + 1);
}

/** The row position a digit code names, or null when the code is not a digit. */
export function navHotkeyIndexForCode(code: string): number | null {
  const index = NAV_HOTKEY_DIGIT_CODES.indexOf(code);
  return index === -1 ? null : index;
}

export const NAV_HOTKEY_ACTION_LABELS: Record<NavHotkeyAction, string> = {
  dashboard: "Dashboard",
  projects: "Projects",
  dms: "Direct messages",
};

/**
 * What each destination chord actually does.
 *
 * Written out rather than generated from the label because two of the three
 * are not simply "go there": a project remembers where you left it, and the
 * direct-messages chord flips to the composer once you are already in a
 * conversation. A settings row that said "Go to direct messages" would be
 * describing a control that behaves differently half the time.
 */
export const NAV_HOTKEY_ACTION_DESCRIPTIONS: Record<NavHotkeyAction, string> = {
  dashboard: "Open the Dashboard.",
  projects: "Open the Projects list.",
  dms: "Open your last direct message — or start a new one if you are already in a conversation.",
};

/**
 * The glyph a modifier wears in the UI. macOS has single-character symbols for
 * all three; elsewhere the spelled-out name is what people recognise.
 */
export function navHotkeyModifierGlyph(
  modifier: NavHotkeyModifier,
  isMac: boolean,
): string {
  if (isMac) {
    return modifier === "alt" ? "⌥" : modifier === "meta" ? "⌘" : "⌃";
  }
  return modifier === "alt" ? "Alt+" : modifier === "meta" ? "Win+" : "Ctrl+";
}

export function navHotkeyModifierLabel(
  modifier: NavHotkeyModifier,
  isMac: boolean,
): string {
  if (modifier === "alt") return isMac ? "Option" : "Alt";
  if (modifier === "meta") return isMac ? "Command" : "Windows";
  return "Control";
}

/**
 * The human-readable name of a `KeyboardEvent.code`.
 *
 * Deliberately derived from the *code*, not `event.key`: with Option held,
 * macOS rewrites `key` to the alternate glyph (⌥D is "∂", ⌥1 is "¡"), so a
 * label built from `key` would show a symbol nobody can find on their
 * keyboard.
 */
export function navHotkeyCodeLabel(code: string): string {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  if (code.startsWith("Numpad")) return `Numpad ${code.slice(6)}`;
  if (code.startsWith("Arrow")) return code.slice(5);
  return code;
}

/**
 * Whether a `KeyboardEvent.code` may be bound to a named action.
 *
 * Letters only. Digits are reserved for positions, and function/punctuation
 * codes vary enough across layouts that a binding made on one keyboard would
 * not reproduce on another.
 */
export function isBindableNavHotkeyCode(code: string): boolean {
  return /^Key[A-Z]$/.test(code);
}

export type NavHotkeyConflict =
  | { kind: "same-modifier" }
  | { kind: "duplicate-code"; code: string; actions: NavHotkeyAction[] };

/**
 * Conflicts *within* the nav hotkeys themselves — the ones that make a chord
 * ambiguous rather than merely crowded. Collisions with the wider read-only
 * shortcut registry are reported separately by the settings card, which has
 * that registry in scope.
 */
export function findNavHotkeyConflicts(
  bindings: NavHotkeyBindings,
): NavHotkeyConflict[] {
  const conflicts: NavHotkeyConflict[] = [];
  if (bindings.scopeModifier === bindings.itemModifier) {
    conflicts.push({ kind: "same-modifier" });
  }

  const byCode = new Map<string, NavHotkeyAction[]>();
  for (const action of NAV_HOTKEY_ACTIONS) {
    const code = bindings.codes[action];
    const existing = byCode.get(code);
    if (existing) existing.push(action);
    else byCode.set(code, [action]);
  }
  for (const [code, actions] of byCode) {
    if (actions.length > 1) {
      conflicts.push({ kind: "duplicate-code", code, actions });
    }
  }

  return conflicts;
}

function isModifier(value: unknown): value is NavHotkeyModifier {
  return value === "alt" || value === "meta" || value === "ctrl";
}

/**
 * Parse a stored payload, falling back field-by-field to the defaults.
 *
 * Field-by-field rather than all-or-nothing: a binding blob that gained a
 * field in a later version should not cost someone the three bindings they
 * already set. Returns null only when the payload is not a v1 object at all.
 */
export function parseNavHotkeyPayload(json: unknown): NavHotkeyBindings | null {
  if (typeof json !== "object" || json === null || Array.isArray(json)) {
    return null;
  }
  const obj = json as Record<string, unknown>;
  if (obj.version !== 1) return null;

  const rawCodes =
    typeof obj.codes === "object" && obj.codes !== null
      ? (obj.codes as Record<string, unknown>)
      : {};

  const codes = { ...DEFAULT_NAV_HOTKEY_BINDINGS.codes };
  for (const action of NAV_HOTKEY_ACTIONS) {
    const code = rawCodes[action];
    if (typeof code === "string" && isBindableNavHotkeyCode(code)) {
      codes[action] = code;
    }
  }

  return {
    version: 1,
    enabled:
      typeof obj.enabled === "boolean"
        ? obj.enabled
        : DEFAULT_NAV_HOTKEY_BINDINGS.enabled,
    scopeModifier: isModifier(obj.scopeModifier)
      ? obj.scopeModifier
      : DEFAULT_NAV_HOTKEY_BINDINGS.scopeModifier,
    itemModifier: isModifier(obj.itemModifier)
      ? obj.itemModifier
      : DEFAULT_NAV_HOTKEY_BINDINGS.itemModifier,
    codes,
  };
}

type NavHotkeyChordEvent = Pick<
  KeyboardEvent,
  "altKey" | "code" | "ctrlKey" | "metaKey" | "shiftKey"
>;

/** Whether exactly `modifier` is down, with no other modifier alongside it. */
export function hasExactModifier(
  event: NavHotkeyChordEvent,
  modifier: NavHotkeyModifier,
): boolean {
  if (event.shiftKey) return false;
  return (
    event.altKey === (modifier === "alt") &&
    event.metaKey === (modifier === "meta") &&
    event.ctrlKey === (modifier === "ctrl")
  );
}

export type NavHotkeyMatch =
  | { kind: "action"; action: NavHotkeyAction }
  | { kind: "scope-position"; index: number }
  | { kind: "item-position"; index: number };

/**
 * Resolve a keydown against the bindings, or null when it is not one of ours.
 *
 * Matching is on `event.code` throughout — see {@link navHotkeyCodeLabel} for
 * why `event.key` cannot be trusted once Option is held.
 */
export function matchNavHotkey(
  event: NavHotkeyChordEvent,
  bindings: NavHotkeyBindings,
): NavHotkeyMatch | null {
  if (!bindings.enabled) return null;

  if (hasExactModifier(event, bindings.scopeModifier)) {
    for (const action of NAV_HOTKEY_ACTIONS) {
      if (bindings.codes[action] === event.code) {
        return { kind: "action", action };
      }
    }
    const index = navHotkeyIndexForCode(event.code);
    if (index !== null) return { kind: "scope-position", index };
    return null;
  }

  if (hasExactModifier(event, bindings.itemModifier)) {
    const index = navHotkeyIndexForCode(event.code);
    if (index !== null) return { kind: "item-position", index };
  }

  return null;
}
