// Shared Owner / Collaborator / Viewer role vocabulary for invite-based
// access to projects, coding sessions, and shared terminals. All three
// surfaces render the same three tiers so role menus look and sort
// identically everywhere; what each tier *means* is entity-specific and
// enforced by the relay (and, for terminals, the owner host) — client role
// state is advisory, for UI affordances only.
//
// Wire values match the pinned vocabularies in crates/beekeeper-core/src/kind.rs
// (PROJECT_ROLES / SHELL_ROLES). Coding sessions carry roles as kind:44228
// transition types instead (grant-operator ⇒ collaborator, grant-viewer ⇒
// viewer); use the mapping helpers where that surface needs them.

export type EntityRole = "owner" | "collaborator" | "viewer";

/// Roles an invitee can be granted. `owner` is only grantable on projects —
/// sessions and terminals have exactly one owner, their creator.
export const PROJECT_GRANTABLE_ROLES: EntityRole[] = [
  "owner",
  "collaborator",
  "viewer",
];
export const SESSION_GRANTABLE_ROLES: EntityRole[] = ["collaborator", "viewer"];

/// Descending-capability sort rank, matching the channel members sidebar's
/// owner-first convention.
export function entityRoleRank(role: EntityRole): number {
  if (role === "owner") return 0;
  if (role === "collaborator") return 1;
  return 2;
}

export const ENTITY_ROLE_LABELS: Record<EntityRole, string> = {
  owner: "Owner",
  collaborator: "Collaborator",
  viewer: "Viewer",
};

/// One-line capability description per role, for role menus.
export const ENTITY_ROLE_DESCRIPTIONS: Record<EntityRole, string> = {
  owner: "Full access, including managing members",
  collaborator: "Can edit and interact",
  viewer: "Read-only access",
};

/// Parse a wire role string (tag element / projection value). Unknown values
/// return undefined — callers decide the surface's fail-direction (display
/// as viewer, never silently grant).
export function parseEntityRole(
  value: string | undefined,
): EntityRole | undefined {
  return value === "owner" || value === "collaborator" || value === "viewer"
    ? value
    : undefined;
}
