/**
 * Collisions between the navigation hotkeys and the app's existing shortcuts.
 *
 * The rest of `KEYBOARD_SHORTCUTS` is fixed, so a rebind that lands on one of
 * them takes it away — quietly, at the window listener, with no error anywhere.
 * Choosing ⌘ as the scope modifier, for instance, puts ⌘D and ⌘P on top of
 * nothing but puts the digit chords next to zoom, and ⌘F/⌘K are one keystroke
 * from the letters people reach for first.
 *
 * Rather than forbid the choice, the settings card names what it would break
 * and lets the person decide. Comparison is on the rendered chord label, which
 * is the same string the registry publishes for the platform, so a shortcut
 * added to that list later is checked without anything here changing.
 */

import {
  KEYBOARD_SHORTCUTS,
  type KeyboardShortcut,
} from "@/shared/lib/keyboard-shortcuts";

import {
  NAV_HOTKEY_ACTIONS,
  NAV_HOTKEY_DIGIT_CODES,
  navHotkeyCodeLabel,
  navHotkeyModifierGlyph,
  type NavHotkeyAction,
  type NavHotkeyBindings,
} from "./navHotkeyBindings";

export type NavHotkeyRegistryConflict = {
  /** The nav hotkey at fault: a named destination, or the positional family. */
  source: NavHotkeyAction | "positions";
  /** The chord both want. */
  chord: string;
  /** The existing shortcut it would take over. */
  shortcut: KeyboardShortcut;
};

function chordLabel(modifierGlyph: string, code: string): string {
  return `${modifierGlyph}${navHotkeyCodeLabel(code)}`;
}

export function findNavHotkeyRegistryConflicts(
  bindings: NavHotkeyBindings,
  isMac: boolean,
): NavHotkeyRegistryConflict[] {
  // Deliberately not `getPlatformKeys`, which reads the platform itself: one
  // `isMac` has to decide both the registry spelling and the glyph the nav
  // chord is rendered with, or the two halves compare strings from different
  // platforms and every collision goes unreported.
  const byChord = new Map<string, KeyboardShortcut>();
  for (const shortcut of KEYBOARD_SHORTCUTS) {
    byChord.set(isMac ? shortcut.keys : shortcut.keysWindows, shortcut);
  }

  const conflicts: NavHotkeyRegistryConflict[] = [];
  const scopeGlyph = navHotkeyModifierGlyph(bindings.scopeModifier, isMac);
  const itemGlyph = navHotkeyModifierGlyph(bindings.itemModifier, isMac);

  for (const action of NAV_HOTKEY_ACTIONS) {
    const chord = chordLabel(scopeGlyph, bindings.codes[action]);
    const shortcut = byChord.get(chord);
    if (shortcut) conflicts.push({ source: action, chord, shortcut });
  }

  // The digits are one family; reporting nine near-identical rows would bury
  // the one that matters, so the first collision stands for the set.
  for (const glyph of new Set([scopeGlyph, itemGlyph])) {
    for (const code of NAV_HOTKEY_DIGIT_CODES) {
      const chord = chordLabel(glyph, code);
      const shortcut = byChord.get(chord);
      if (shortcut) {
        conflicts.push({ source: "positions", chord, shortcut });
        break;
      }
    }
  }

  return conflicts;
}
