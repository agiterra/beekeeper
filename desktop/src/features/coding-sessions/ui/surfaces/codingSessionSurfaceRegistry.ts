import * as React from "react";
import { Sparkles } from "lucide-react";

import type {
  CodingSessionSurfaceAvailability,
  CodingSessionSurfaceBaseCtx,
  CodingSessionSurfaceCtx,
  CodingSessionSurfaceLens,
} from "./codingSessionSurfaceContext";

/**
 * The session view's surface registry (SV-38).
 *
 * A surface is one definition: its id, label, icon, launcher letter, order,
 * where it opens, which lenses list it, whether it can open now (and why
 * not), its badge and its panel. The launcher, the tab strip, the drawer, the
 * shortcuts, the dimming and the badge slots all read definitions and nothing
 * else, so a new surface — Memory, say — is one file under `ui/surfaces/`
 * plus one line in `codingSessionBuiltinSurfaces.ts` (DB1).
 *
 * **The registry holds definitions, not community data.** A definition is
 * static code fixed at module load; everything about a session arrives
 * through the `ctx` argument at render time. So a registry never needs a
 * reset in `resetCommunityState()`: switching community changes the `ctx`
 * the definitions are called with, never the definitions.
 */

/** Where a surface opens: a tab in the right panel, or the bottom drawer. */
export type CodingSessionSurfacePlacement = "right" | "drawer";

/** Where a badge is drawn, so one Badge component can size itself. */
export type CodingSessionSurfaceBadgeSlot = "launcher" | "tab" | "header";

export type CodingSessionSurfaceIcon = React.ComponentType<{
  className?: string;
  "aria-hidden"?: boolean;
}>;

export type CodingSessionSurfaceDefinition = {
  /** Stable id; the tab's testid is `coding-session-surface-tab-<id>`. */
  id: string;
  label: string;
  icon: CodingSessionSurfaceIcon;
  /** One letter, A–Z. Bare while the launcher is visible (SV-21). */
  shortcut: string;
  /** Launcher and "+" menu order, ascending. */
  order: number;
  placement: CodingSessionSurfacePlacement;
  /** The lenses that list this surface. Not listing is a lens choice. */
  lenses: readonly CodingSessionSurfaceLens[];
  /** Pure: whether the surface can open now, or the sentence why not. */
  availability: (
    ctx: CodingSessionSurfaceCtx,
  ) => CodingSessionSurfaceAvailability;
  /** Live badge; a component, so it may use hooks. One per row or tab. */
  Badge?: React.ComponentType<{
    ctx: CodingSessionSurfaceCtx;
    slot: CodingSessionSurfaceBadgeSlot;
  }>;
  Panel: React.ComponentType<{ ctx: CodingSessionSurfaceCtx }>;
  /**
   * Optional React hook, called once per view for every registered surface
   * in registry order (stable, because the registry is fixed at load). Its
   * result is `ctx.extensions[id]`, so a surface can compute a value once and
   * share it between its badge and its panel.
   */
  readExtension?: (ctx: CodingSessionSurfaceBaseCtx) => unknown;
};

/**
 * Letters held for surfaces that do not exist yet. A built-in may not take
 * one; only the surface named here may (DB2: Y is Memory's).
 */
export const CODING_SESSION_RESERVED_SURFACE_LETTERS: Readonly<
  Record<string, string>
> = Object.freeze({ Y: "memory" });

const SURFACE_ID = /^[a-z][a-z0-9-]*$/;
const SURFACE_LETTER = /^[A-Z]$/;

export type CodingSessionSurfaceRegistry = {
  /** Every definition, in launcher order. */
  readonly definitions: readonly CodingSessionSurfaceDefinition[];
  /** The definitions a lens lists, in launcher order. */
  forLens(
    lens: CodingSessionSurfaceLens,
  ): readonly CodingSessionSurfaceDefinition[];
  get(id: string): CodingSessionSurfaceDefinition | null;
};

/**
 * Build a registry, refusing anything ambiguous at load: an id or a letter
 * used twice, a malformed id or letter, a reserved letter taken by the wrong
 * surface, or a surface no lens lists.
 */
export function createCodingSessionSurfaceRegistry(
  definitions: readonly CodingSessionSurfaceDefinition[],
): CodingSessionSurfaceRegistry {
  const byId = new Map<string, CodingSessionSurfaceDefinition>();
  const byLetter = new Map<string, string>();
  for (const definition of definitions) {
    if (!SURFACE_ID.test(definition.id)) {
      throw new Error(`Surface id "${definition.id}" is not a lowercase slug.`);
    }
    if (byId.has(definition.id)) {
      throw new Error(`Surface id "${definition.id}" is registered twice.`);
    }
    if (!SURFACE_LETTER.test(definition.shortcut)) {
      throw new Error(
        `Surface "${definition.id}" needs one capital letter, not "${definition.shortcut}".`,
      );
    }
    const holder = byLetter.get(definition.shortcut);
    if (holder !== undefined) {
      throw new Error(
        `Letter ${definition.shortcut} is taken by "${holder}"; "${definition.id}" cannot use it.`,
      );
    }
    const reservedFor =
      CODING_SESSION_RESERVED_SURFACE_LETTERS[definition.shortcut];
    if (reservedFor !== undefined && reservedFor !== definition.id) {
      throw new Error(
        `Letter ${definition.shortcut} is reserved for "${reservedFor}".`,
      );
    }
    if (definition.lenses.length === 0) {
      throw new Error(`Surface "${definition.id}" is listed in no lens.`);
    }
    byId.set(definition.id, definition);
    byLetter.set(definition.shortcut, definition.id);
  }
  const ordered = Object.freeze(
    [...definitions].sort(
      (left, right) =>
        left.order - right.order || left.id.localeCompare(right.id),
    ),
  );
  const lensLists = new Map<
    CodingSessionSurfaceLens,
    readonly CodingSessionSurfaceDefinition[]
  >();
  for (const lens of ["conversation", "mission"] as const) {
    lensLists.set(
      lens,
      Object.freeze(
        ordered.filter((definition) => definition.lenses.includes(lens)),
      ),
    );
  }
  return {
    definitions: ordered,
    forLens: (lens) => lensLists.get(lens) ?? [],
    get: (id) => byId.get(id) ?? null,
  };
}

/** A definition with its availability resolved against one `ctx`. */
export type CodingSessionResolvedSurface = {
  definition: CodingSessionSurfaceDefinition;
  availability: CodingSessionSurfaceAvailability;
};

/** Resolve availability once per render for a list of definitions. */
export function resolveCodingSessionSurfaces(
  definitions: readonly CodingSessionSurfaceDefinition[],
  ctx: CodingSessionSurfaceCtx,
): CodingSessionResolvedSurface[] {
  return definitions.map((definition) => ({
    definition,
    availability: definition.availability(ctx),
  }));
}

// ---------------------------------------------------------------------------
// E2E-only extra surfaces (SV-38's screenshot): plain data a spec sets on
// `window.__BEEKEEPER_E2E_EXTRA_SURFACES__` before the app loads. Read only in a
// `--mode e2e` build; every other build ignores the global entirely.
// ---------------------------------------------------------------------------

/** What a spec may declare. Data only: the registry supplies the components. */
export type CodingSessionE2eExtraSurface = {
  id: string;
  label: string;
  shortcut: string;
  order?: number;
  lenses?: readonly CodingSessionSurfaceLens[];
  /** Absent means available. */
  unavailableReason?: string;
  /** An activity badge count to draw; absent or 0 draws none. */
  badgeCount?: number;
  /** The placeholder text the panel shows. */
  panelText?: string;
};

function isE2eBuild(): boolean {
  // Vite replaces only the literal `import.meta.env.MODE`; any other spelling
  // ships to the bundle unreplaced and reads undefined at runtime. Under the
  // Node unit-test runner `import.meta.env` is absent, so the read throws.
  try {
    return import.meta.env.MODE === "e2e";
  } catch {
    return false;
  }
}

/** Turn declared data into a definition. Exported for the unit test. */
export function codingSessionE2eExtraSurfaceDefinition(
  extra: CodingSessionE2eExtraSurface,
): CodingSessionSurfaceDefinition {
  const count = extra.badgeCount ?? 0;
  const ExtraBadge = () =>
    count > 0
      ? React.createElement(
          "span",
          {
            "aria-label": `${count} ${extra.label.toLowerCase()} items active`,
            className:
              "flex h-3.5 min-w-3.5 items-center justify-center rounded-full bg-primary px-1 text-3xs font-semibold leading-none text-primary-foreground tabular-nums",
            "data-testid": `coding-session-surface-badge-${extra.id}`,
            "data-tone": "activity",
          },
          String(count),
        )
      : null;
  const ExtraPanel = () =>
    React.createElement(
      "div",
      {
        className: "p-4 text-sm text-muted-foreground",
        "data-testid": `coding-session-surface-panel-${extra.id}`,
      },
      extra.panelText ?? `${extra.label} is a test surface.`,
    );
  return {
    id: extra.id,
    label: extra.label,
    icon: Sparkles,
    shortcut: extra.shortcut,
    order: extra.order ?? 950,
    placement: "right",
    lenses: extra.lenses ?? ["conversation", "mission"],
    availability: () =>
      extra.unavailableReason
        ? { available: false, reason: extra.unavailableReason }
        : { available: true },
    Badge: count > 0 ? ExtraBadge : undefined,
    Panel: ExtraPanel,
  };
}

/** The e2e build's extra surfaces, or none in any other build. */
export function readCodingSessionE2eExtraSurfaces(): CodingSessionSurfaceDefinition[] {
  if (!isE2eBuild() || typeof window === "undefined") return [];
  const declared = (
    window as Window & { __BEEKEEPER_E2E_EXTRA_SURFACES__?: unknown }
  ).__BEEKEEPER_E2E_EXTRA_SURFACES__;
  if (!Array.isArray(declared)) return [];
  return declared.map((extra) =>
    codingSessionE2eExtraSurfaceDefinition(
      extra as CodingSessionE2eExtraSurface,
    ),
  );
}
