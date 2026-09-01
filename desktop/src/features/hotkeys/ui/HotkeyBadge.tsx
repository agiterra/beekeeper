import * as React from "react";

import { cn } from "@/shared/lib/cn";
import { isMacPlatform } from "@/shared/lib/platform";
import {
  getHeldModifier,
  getHeldModifierServerSnapshot,
  subscribeHeldModifier,
} from "@/features/hotkeys/lib/heldModifierStore";
import {
  navHotkeyCodeLabel,
  navHotkeyDigitLabel,
  navHotkeyModifierGlyph,
  type NavHotkeyAction,
  type NavHotkeyModifier,
} from "@/features/hotkeys/lib/navHotkeyBindings";
import { useNavHotkeyBindings } from "@/features/hotkeys/lib/navHotkeyBindingsStore";

/** The armed modifier, or null. Subscribing here keeps the re-render at the leaf. */
export function useHeldModifier(): NavHotkeyModifier | null {
  return React.useSyncExternalStore(
    subscribeHeldModifier,
    getHeldModifier,
    getHeldModifierServerSnapshot,
  );
}

/**
 * The `data-hotkeys-armed` flag the badge stylesheet keys off.
 *
 * A prop bag rather than a wrapper element, so it can be applied to a
 * component that already owns its DOM node — and so the subscription lives on
 * the sidebar root instead of on every row.
 */
export function useHotkeyArmedProps(): { "data-hotkeys-armed"?: "" } {
  const held = useHeldModifier();
  const { enabled } = useNavHotkeyBindings();
  // With the hotkeys switched off there are no badges to make room for, so
  // dimming the row's counts and actions would hide them for nothing.
  return held && enabled ? { "data-hotkeys-armed": "" } : {};
}

type HotkeyBadgeProps = {
  /** Which held modifier reveals this badge. */
  modifier: NavHotkeyModifier;
  /** What the chord sends — a digit for a position, a letter for a destination. */
  label: string | null;
  /**
   * Sit in the flow instead of overlaying the row's right edge. Section
   * headings are `w-fit` flex rows with a hover chevron; an absolute badge
   * there would land over the section's action menu rather than the heading.
   */
  inline?: boolean;
  testId?: string;
};

/**
 * The chord hint that appears over a row while its modifier is held.
 *
 * Renders nothing at all when the modifier is not armed: an always-mounted
 * hidden span would still cost a subscription per row on every keypress, and
 * the badge overlaps the row's own trailing furniture, so it must not reserve
 * layout when it is not showing.
 */
export function HotkeyBadge({
  inline = false,
  label,
  modifier,
  testId,
}: HotkeyBadgeProps) {
  const held = useHeldModifier();
  if (label === null || held !== modifier) return null;

  const glyph = navHotkeyModifierGlyph(modifier, isMacPlatform());

  return (
    <span
      aria-hidden
      className={cn(
        "pointer-events-none rounded border border-border/70 bg-muted px-1 py-px",
        "font-mono text-2xs leading-none text-muted-foreground shadow-xs",
        inline
          ? "ml-1 shrink-0"
          : "absolute right-1.5 top-1/2 z-10 -translate-y-1/2",
      )}
      data-hotkey-badge=""
      data-testid={testId}
    >
      {glyph}
      {label}
    </span>
  );
}

/**
 * The badge over a top-level destination — Dashboard, Projects, Direct
 * messages — showing the letter currently bound to it.
 */
export function ScopeActionBadge({
  action,
  inline,
  testId,
}: {
  action: NavHotkeyAction;
  inline?: boolean;
  testId?: string;
}) {
  const bindings = useNavHotkeyBindings();
  return (
    <HotkeyBadge
      inline={inline}
      label={
        bindings.enabled ? navHotkeyCodeLabel(bindings.codes[action]) : null
      }
      modifier={bindings.scopeModifier}
      testId={testId ?? `hotkey-badge-${action}`}
    />
  );
}

/** The badge over the nth project, numbered by position. */
export function ScopePositionBadge({
  index,
  testId,
}: {
  index: number;
  testId?: string;
}) {
  const bindings = useNavHotkeyBindings();
  return (
    <HotkeyBadge
      label={bindings.enabled ? navHotkeyDigitLabel(index) : null}
      modifier={bindings.scopeModifier}
      testId={testId ?? `hotkey-badge-project-${index + 1}`}
    />
  );
}

/** The badge over the nth row of the surface currently open. */
export function ItemPositionBadge({
  index,
  testId,
}: {
  index: number | null;
  testId?: string;
}) {
  const bindings = useNavHotkeyBindings();
  if (index === null || !bindings.enabled) return null;
  return (
    <HotkeyBadge
      label={navHotkeyDigitLabel(index)}
      modifier={bindings.itemModifier}
      testId={testId ?? `hotkey-badge-item-${index + 1}`}
    />
  );
}
