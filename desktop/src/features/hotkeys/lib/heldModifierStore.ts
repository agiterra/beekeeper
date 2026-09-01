/**
 * Which hotkey modifier is being *held* right now, if any.
 *
 * A module-level external store rather than React context on purpose: every
 * sidebar row wants to know, and a context value that flips on ⌘-down would
 * re-render the whole sidebar every time someone reached for ⌘K. Only the leaf
 * badge components subscribe, so a modifier press repaints a few spans.
 *
 * Four rules keep it honest:
 *
 * - **Arm on a deliberate hold, not a tap.** ⌘K, ⌘F and ⌘, all start with ⌘
 *   down; flashing the whole badge set for 80ms on each of them is strobing,
 *   not affordance. The modifier must be held alone past {@link ARM_DELAY_MS}.
 * - **Disarm on the first other key.** Once the chord turns out to be ⌘K, the
 *   badges are wrong and must go immediately.
 * - **Disarm on blur.** ⌘-Tab sends keydown and then takes the window away, so
 *   the matching keyup never arrives. Without this the badges stay painted on
 *   forever, over a keyboard that is no longer holding anything.
 * - **Never arm under a modal.** A dialog owns the keyboard while it is up,
 *   and numbering rows behind an overlay points at things nobody can click.
 */

import type { NavHotkeyModifier } from "./navHotkeyBindings";

export const ARM_DELAY_MS = 250;

type Listener = () => void;

const listeners = new Set<Listener>();

let armed: NavHotkeyModifier | null = null;
let pending: NavHotkeyModifier | null = null;
let pendingTimer: ReturnType<typeof setTimeout> | null = null;

/**
 * Whether a modal dialog is up.
 *
 * Read from the DOM rather than tracked through a flag some overlay has to
 * remember to set: a missed call there would leave the badges arming under a
 * dialog with no symptom until someone noticed the numbers pointing at rows
 * behind the scrim. Radix stamps `data-state` on the open content, so the
 * question can simply be asked.
 */
function isModalOpen(): boolean {
  if (typeof document === "undefined") return false;
  return document.querySelector('[role="dialog"][data-state="open"]') !== null;
}

function emit(next: NavHotkeyModifier | null): void {
  if (armed === next) return;
  armed = next;
  for (const listener of listeners) listener();
}

function clearPending(): void {
  if (pendingTimer !== null) {
    clearTimeout(pendingTimer);
    pendingTimer = null;
  }
  pending = null;
}

function disarm(): void {
  clearPending();
  emit(null);
}

function modifierFromEvent(
  event: Pick<KeyboardEvent, "altKey" | "ctrlKey" | "metaKey" | "shiftKey">,
): NavHotkeyModifier | null {
  // Shift is never a hotkey modifier here, and a modifier is only "held alone"
  // when it is the single one down — ⌥⌘ is a different chord, not either one.
  if (event.shiftKey) return null;
  const down = [
    event.altKey ? ("alt" as const) : null,
    event.metaKey ? ("meta" as const) : null,
    event.ctrlKey ? ("ctrl" as const) : null,
  ].filter((value): value is NavHotkeyModifier => value !== null);
  return down.length === 1 ? down[0] : null;
}

/** True when the pressed key is itself a modifier key rather than a chord key. */
function isModifierKey(key: string): boolean {
  return (
    key === "Alt" || key === "Meta" || key === "Control" || key === "Shift"
  );
}

function handleKeyDown(event: KeyboardEvent): void {
  if (isModalOpen()) {
    disarm();
    return;
  }

  if (!isModifierKey(event.key)) {
    // A real key joined the chord. Whatever it turns out to be — ⌘K, or one of
    // our own digits — the hold is over and the badges have done their job.
    disarm();
    return;
  }

  if (event.repeat) return;

  const modifier = modifierFromEvent(event);
  if (!modifier) {
    disarm();
    return;
  }

  if (armed === modifier || pending === modifier) return;

  clearPending();
  pending = modifier;
  pendingTimer = setTimeout(() => {
    pendingTimer = null;
    const held = pending;
    pending = null;
    if (held && !isModalOpen()) emit(held);
  }, ARM_DELAY_MS);
}

function handleKeyUp(event: KeyboardEvent): void {
  const modifier = modifierFromEvent(event);
  if (modifier === null || (armed !== null && modifier !== armed)) {
    disarm();
    return;
  }
  if (pending !== null && modifier !== pending) disarm();
}

let installed = 0;

/** Subscribe to arm/disarm transitions. Installs the window listeners lazily. */
export function subscribeHeldModifier(listener: Listener): () => void {
  listeners.add(listener);
  if (installed === 0 && typeof window !== "undefined") {
    window.addEventListener("keydown", handleKeyDown, { capture: true });
    window.addEventListener("keyup", handleKeyUp, { capture: true });
    window.addEventListener("blur", disarm);
    document.addEventListener("visibilitychange", disarm);
  }
  installed += 1;

  return () => {
    listeners.delete(listener);
    installed -= 1;
    if (installed === 0 && typeof window !== "undefined") {
      window.removeEventListener("keydown", handleKeyDown, { capture: true });
      window.removeEventListener("keyup", handleKeyUp, { capture: true });
      window.removeEventListener("blur", disarm);
      document.removeEventListener("visibilitychange", disarm);
      disarm();
    }
  };
}

export function getHeldModifier(): NavHotkeyModifier | null {
  return armed;
}

/** Server-render snapshot: nothing is ever held before hydration. */
export function getHeldModifierServerSnapshot(): NavHotkeyModifier | null {
  return null;
}

/** Test-only: drop all state between cases. */
export function __resetHeldModifierForTests(): void {
  clearPending();
  armed = null;
}
