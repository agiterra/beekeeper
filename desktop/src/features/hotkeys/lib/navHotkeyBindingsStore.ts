/**
 * The live navigation hotkey bindings, shared by the dispatcher and settings.
 *
 * A module-level store because the two readers are on opposite sides of the
 * shell: `useNavigationHotkeys` runs in `AppShell`, and the settings card that
 * edits the bindings is rendered in place of the outlet. Threading a setter
 * between them would mean widening three prop chains to move one small object;
 * a store lets an edit in settings take effect on the next keypress with no
 * remount and no reload.
 *
 * Community-scoped, so `resetCommunityState()` clears it on a community
 * switch — see `features/communities/useCommunityInit.ts`.
 */

import * as React from "react";

import {
  DEFAULT_NAV_HOTKEY_BINDINGS,
  type NavHotkeyBindings,
} from "./navHotkeyBindings";
import {
  navHotkeyStorageKey,
  readNavHotkeyBindings,
  writeNavHotkeyBindings,
} from "./navHotkeyStorage";

type Listener = () => void;

const listeners = new Set<Listener>();

let bindings: NavHotkeyBindings = DEFAULT_NAV_HOTKEY_BINDINGS;
let identity: { pubkey: string; relayUrl?: string } | null = null;

function emit(next: NavHotkeyBindings): void {
  if (next === bindings) return;
  bindings = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function getNavHotkeyBindings(): NavHotkeyBindings {
  return bindings;
}

/** Read the stored bindings for an identity and make them current. */
export function loadNavHotkeyBindings(
  pubkey: string | undefined,
  relayUrl?: string,
): void {
  if (!pubkey) {
    identity = null;
    emit(DEFAULT_NAV_HOTKEY_BINDINGS);
    return;
  }
  identity = { pubkey, relayUrl };
  emit(readNavHotkeyBindings(pubkey, relayUrl));
}

/**
 * Persist and publish new bindings. A write that localStorage refuses leaves
 * the previous bindings in place rather than showing a change that will be
 * gone on the next launch.
 */
export function updateNavHotkeyBindings(next: NavHotkeyBindings): boolean {
  if (!identity) {
    emit(next);
    return false;
  }
  if (!writeNavHotkeyBindings(identity.pubkey, next, identity.relayUrl)) {
    return false;
  }
  emit(next);
  return true;
}

/** Clear back to defaults; called on community switch. */
export function resetNavHotkeyBindings(): void {
  identity = null;
  emit(DEFAULT_NAV_HOTKEY_BINDINGS);
}

/** The current bindings, re-rendering the caller when they change. */
export function useNavHotkeyBindings(): NavHotkeyBindings {
  return React.useSyncExternalStore(
    subscribe,
    getNavHotkeyBindings,
    getNavHotkeyBindings,
  );
}

/**
 * Load the bindings for the signed-in identity and mirror edits made in
 * another window. Mount once, in the shell.
 */
export function useNavHotkeyBindingsSync(
  pubkey: string | undefined,
  relayUrl?: string,
): void {
  React.useEffect(() => {
    loadNavHotkeyBindings(pubkey, relayUrl);
  }, [pubkey, relayUrl]);

  React.useEffect(() => {
    if (!pubkey) return;
    const key = navHotkeyStorageKey(pubkey, relayUrl);
    const handler = (event: StorageEvent) => {
      if (event.key !== key) return;
      emit(readNavHotkeyBindings(pubkey, relayUrl));
    };
    window.addEventListener("storage", handler);
    return () => {
      window.removeEventListener("storage", handler);
    };
  }, [pubkey, relayUrl]);
}
