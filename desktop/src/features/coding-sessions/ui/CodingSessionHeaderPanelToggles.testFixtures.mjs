/**
 * A fake surface shell for the header's unit tests: a `ctx` carrying only
 * what the header reads (panel state and actions), resolved surfaces whose
 * badges draw a fixed tone, and the header's panel props.
 *
 * Not a test file itself (no `.test.`), so the runner never executes it.
 */
import React from "react";

function Icon() {
  return null;
}

/** A Badge that draws one pill with `tone` (or no `data-tone` at all). */
export function toneBadge(tone, description = null) {
  return function FixtureBadge() {
    if (tone === null) return null;
    return React.createElement(
      "span",
      {
        "aria-label": description ?? undefined,
        "data-tone": tone === "untoned" ? undefined : tone,
      },
      "•",
    );
  };
}

export const CLOSED_PANELS = {
  rightOpen: false,
  tabs: [],
  active: null,
  expanded: false,
  bottomOpen: false,
  userActed: false,
};

/**
 * @param {object} input
 * @param {Array<{id: string, label?: string, placement?: "right" | "drawer",
 *   Badge?: Function, available?: boolean, reason?: string}>} input.surfaces
 * @param {object} [input.panelState]
 * @param {string | null} [input.bottomUnavailableReason]
 * @param {Map<string, number>} [input.calls]
 */
export function fakeSurfaceShell({
  surfaces,
  panelState = CLOSED_PANELS,
  bottomUnavailableReason = null,
  calls = new Map(),
}) {
  const count = (name) => () => calls.set(name, (calls.get(name) ?? 0) + 1);
  const actions = {
    open: (id) => count(`open:${id}`)(),
    openProactive: count("openProactive"),
    toggle: (id) => count(`toggle:${id}`)(),
    close: (id) => count(`close:${id}`)(),
    closeMany: count("closeMany"),
    activate: (id) => count(`activate:${id}`)(),
    closeRight: count("closeRight"),
    toggleRight: count("toggleRight"),
    toggleBottom: count("toggleBottom"),
    toggleExpanded: count("toggleExpanded"),
  };
  const ctx = {
    panelState,
    panels: actions,
    activeSurfaceId: panelState.rightOpen ? panelState.active : null,
    extensions: {},
  };
  const resolved = surfaces.map((surface, index) => ({
    definition: {
      id: surface.id,
      label: surface.label ?? surface.id,
      icon: Icon,
      shortcut: String.fromCharCode(65 + index),
      order: index,
      placement: surface.placement ?? "right",
      lenses: ["conversation", "mission"],
      availability: () => ({ available: true }),
      Badge: surface.Badge,
      Panel: Icon,
    },
    availability:
      surface.available === false
        ? { available: false, reason: surface.reason ?? "Not here." }
        : { available: true },
  }));
  return {
    calls,
    shell: {
      ctx,
      panels: { state: panelState, actions },
      surfaces: resolved,
      headerPanels: {
        rightOpen: panelState.rightOpen,
        onToggleRight: actions.toggleRight,
        bottomOpen: panelState.bottomOpen,
        onToggleBottom: actions.toggleBottom,
        bottomUnavailableReason,
      },
    },
  };
}
