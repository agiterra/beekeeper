/**
 * The wire shape of `list_project_role_packs`: one project's role packs as
 * the staging ladder this computer would walk for it, one row per role.
 *
 * Mirrors the Rust `RolePackSummary` (serde `rename_all = "camelCase"`) in
 * `desktop/src-tauri/src/managed_agents/role_packs_view.rs`. Do not widen it
 * here — a field the backend never sends would render as an empty fact.
 */

/**
 * Which rung of the staging ladder the pack comes from — the
 * `SeatPackOrigin` vocabulary verbatim. `shipped` is a real answer, not a
 * default to hide behind a friendlier label.
 */
export type RolePackOrigin = "project" | "checkout" | "installed" | "shipped";

/**
 * One skill as its `SKILL.md` frontmatter names it. A skill is in this list
 * only because that file was read; nothing is listed from a directory name.
 */
export type RolePackSkill = {
  name: string;
  /** `""` when the frontmatter names no description. */
  description: string;
  /** `true` when the persona did not claim this skill and the pack shares it. */
  shared: boolean;
};

/** Exactly what staging would stamp on a seat. Never guessed: `null` when unknown. */
export type RolePackRef = {
  /** The packs repository coordinate, or `app:shipped` for bundled packs. */
  repo: string;
  /** The 40-hex commit, or the app's own version string for shipped packs. */
  sha: string;
};

/** One role, from the rung staging would pick for the project. */
export type RolePackSummary = {
  /** Role slug, e.g. `lead`. */
  role: string;
  displayName: string;
  /** Persona frontmatter description; `""` when absent. */
  description: string;
  /** First paragraph of the persona prompt, at most 400 characters; `""` when none. */
  summary: string;
  /** `.plugin/plugin.json` version, or `null` when the pack declares none. */
  version: string | null;
  origin: RolePackOrigin;
  /** Absolute path of the directory that would be staged. */
  packDir: string;
  packRef: RolePackRef | null;
  skills: RolePackSkill[];
  /** The backend's sentence for why this role cannot be staged for this project, or `null`. */
  refusal: string | null;
};
