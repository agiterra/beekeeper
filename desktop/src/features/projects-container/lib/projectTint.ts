import type * as React from "react";

/**
 * The element-local custom property carrying a project's tint. Consumed by
 * theme.css: the sidebar group and the content surface each mix it over
 * their own background at a low percentage, so one hex works in both light
 * and dark mode. Scoped per element — never set on :root (see the sidebar
 * override block in theme.css for why).
 */
export const PROJECT_TINT_VAR = "--project-tint";

/** Inline style carrying the tint var, or undefined when the project has no
 * color — so the attribute and the var appear and disappear together. */
export function projectTintVars(
  color: string | null,
): React.CSSProperties | undefined {
  if (!color) return undefined;
  return { [PROJECT_TINT_VAR]: color } as React.CSSProperties;
}
