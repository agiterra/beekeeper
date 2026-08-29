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

/**
 * The worktree name a team launch suggests for its lead.
 *
 * Suffixed with the seat it belongs to, because a team's directories sit side
 * by side: the lead's tree next to the ones the seats it hires get. Without
 * the suffix the lead's tree and the session's own name are the same string,
 * and the person reading `~/Projects` cannot tell which directory holds whom.
 *
 * Returns `""` when the session name reduces to nothing addressable — the same
 * "no fallback slug" rule as {@link codingSessionWorktreeSlug}, because
 * `-lead` alone names nothing.
 */
export function codingSessionLeadWorktreeName(sessionName: string): string {
  const slug = codingSessionWorktreeSlug(sessionName);
  return slug.length === 0 ? "" : `${slug}-lead`;
}
