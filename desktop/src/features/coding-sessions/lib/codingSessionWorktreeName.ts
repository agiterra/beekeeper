/**
 * The worktree name a session name suggests.
 *
 * This is a *prefill*, not the answer. The host recomputes the slug when it
 * plans or creates the worktree, and only the host can know whether the name
 * is already taken — so the value this produces is what appears in an
 * editable field, and
 * `@/shared/api/tauriCodingSessionWorktrees`.planCodingSessionWorktree says
 * where it would actually land.
 *
 * Kept byte-for-byte in step with `worktree_slug` in
 * `desktop/src-tauri/src/coding_sessions/worktree.rs`: same casing, same
 * separator collapsing, same length cap. A drift here shows up as a preview
 * that names a different directory than the one created.
 */

/** Matches `MAX_WORKTREE_SLUG_LEN` in the Rust module. */
export const MAX_CODING_SESSION_WORKTREE_SLUG_LENGTH = 48;

/**
 * Reduce a free-text name to the slug a directory and a branch can share.
 *
 * Returns `""` when nothing addressable survives — an empty name, pure
 * punctuation, or non-ASCII text. There is no fallback slug on purpose:
 * inventing one would name a branch after nothing.
 */
export function codingSessionWorktreeSlug(name: string): string {
  let slug = "";
  for (const character of name) {
    if (/[A-Za-z0-9]/.test(character)) {
      slug += character.toLowerCase();
    } else if (!slug.endsWith("-")) {
      slug += "-";
    }
  }
  return trimHyphens(
    trimHyphens(slug).slice(0, MAX_CODING_SESSION_WORKTREE_SLUG_LENGTH),
  );
}

function trimHyphens(value: string): string {
  return value.replace(/^-+/, "").replace(/-+$/, "");
}
