/**
 * Device-local persistence for the navigation hotkey bindings.
 *
 * Keyed by pubkey and relay like the sidebar preference stores, so two
 * communities on one machine keep separate bindings and a shared machine does
 * not hand one person's chords to the next. Deliberately *not* synced to the
 * relay: a hotkey is a property of the keyboard in front of you, and a layout
 * bound on a Mac would be wrong on the Windows box it synced to.
 */

import { getStorageItem, setStorageItem } from "@/shared/lib/safeStorage";
import { normalizeRelayUrl } from "@/shared/lib/normalizeRelayUrl";

import {
  DEFAULT_NAV_HOTKEY_BINDINGS,
  parseNavHotkeyPayload,
  type NavHotkeyBindings,
} from "./navHotkeyBindings";

const STORAGE_KEY_PREFIX = "buzz-nav-hotkeys.v1";

export function navHotkeyStorageKey(pubkey: string, relayUrl?: string): string {
  if (!relayUrl) return `${STORAGE_KEY_PREFIX}:${pubkey}`;
  const normalized = normalizeRelayUrl(relayUrl);
  return `${STORAGE_KEY_PREFIX}:${pubkey}:${encodeURIComponent(normalized)}`;
}

export function readNavHotkeyBindings(
  pubkey: string,
  relayUrl?: string,
): NavHotkeyBindings {
  const raw = getStorageItem(navHotkeyStorageKey(pubkey, relayUrl));
  if (!raw) return DEFAULT_NAV_HOTKEY_BINDINGS;
  try {
    return (
      parseNavHotkeyPayload(JSON.parse(raw)) ?? DEFAULT_NAV_HOTKEY_BINDINGS
    );
  } catch {
    return DEFAULT_NAV_HOTKEY_BINDINGS;
  }
}

export function writeNavHotkeyBindings(
  pubkey: string,
  bindings: NavHotkeyBindings,
  relayUrl?: string,
): boolean {
  return setStorageItem(
    navHotkeyStorageKey(pubkey, relayUrl),
    JSON.stringify(bindings),
  );
}
