/**
 * Where a project's code repository is cloned on this computer.
 *
 * The clone lands at `<parent>/<slug>`: `project_agents_init` takes the
 * PARENT (`checkoutParent`) and names the folder after the repository, so a
 * folder row that shows the destination must be able to say which part of
 * it the person chose. These helpers keep that split honest in both
 * directions — joining a parent and a slug into the path the row displays,
 * and reading the parent back out of whatever the person typed. The default
 * parent itself comes from `useDefaultRepositoryFolder`.
 */

/** The host's fallback when no community folder is set (`project_repo_paths.rs`). */
export const HOST_DEFAULT_REPOS_ROOT_LABEL = "~/.beekeeper/REPOS";

/** Strip trailing separators, keeping a bare root (`/`) intact. */
function trimTrailingSeparators(path: string): string {
  const trimmed = path.trim();
  if (trimmed.length <= 1) return trimmed;
  return trimmed.replace(/[\\/]+$/, "") || trimmed.slice(0, 1);
}

/**
 * `<parent>/<slug>`, or `""` when either part is missing — an empty row is
 * an honest "nothing chosen yet", where `/rpg-test` or `~/Code/` would read
 * as a real destination.
 */
export function projectCheckoutPath(
  parent: string | null | undefined,
  slug: string,
): string {
  const base = parent ? trimTrailingSeparators(parent) : "";
  const leaf = slug.trim();
  if (base.length === 0 || leaf.length === 0) return "";
  const separator = base.includes("\\") && !base.includes("/") ? "\\" : "/";
  return base.endsWith(separator)
    ? `${base}${leaf}`
    : `${base}${separator}${leaf}`;
}

/**
 * The parent a typed folder row means. Text ending in `/<slug>` is the
 * destination, so its parent is the prefix; anything else is taken as the
 * parent itself (the clone still lands at `<text>/<slug>`, which the row's
 * helper line says). Empty text is `null`: the host's default root.
 */
export function checkoutParentFromPath(
  text: string,
  slug: string,
): string | null {
  const trimmed = trimTrailingSeparators(text);
  if (trimmed.length === 0) return null;
  const leaf = slug.trim();
  if (leaf.length > 0) {
    for (const separator of ["/", "\\"]) {
      const suffix = `${separator}${leaf}`;
      // `/<slug>` on its own is a destination directly under the root.
      if (trimmed.endsWith(suffix)) {
        const parent = trimmed.slice(0, -suffix.length);
        return parent.length === 0 ? separator : parent;
      }
    }
  }
  return trimmed;
}
