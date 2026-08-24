/**
 * Standing explanation of who can reach a repository created or imported
 * into a project.
 *
 * The relay authorizes git clone/fetch/push against the project's roster:
 * owners push as owners, collaborators as members, viewers read only. That
 * replaced the "pick an access channel" dropdown these dialogs used to show
 * — the roster already was the membership mechanism, and asking for a
 * channel on top of it made a second, parallel ACL the user had to keep in
 * sync by hand.
 *
 * The empty-roster case is stated rather than hidden. A project whose roster
 * is just the viewer produces a repository nobody else can clone, and the
 * honest moment to say so is before the repo exists — not when a teammate's
 * clone comes back 404.
 */
export function ProjectRepoAccessNote({
  otherMemberCount,
  projectName,
}: {
  /** Roster size excluding the viewer; `null` while it is still loading. */
  otherMemberCount: number | null;
  projectName: string;
}) {
  return (
    <div className="space-y-1.5" data-testid="project-repo-access-note">
      <p className="text-sm font-medium text-foreground">Access</p>
      <p className="text-xs text-muted-foreground">
        Members of {projectName} can clone this repository. Owners and
        collaborators can push.
      </p>
      {otherMemberCount === 0 ? (
        <p
          className="text-xs text-muted-foreground"
          data-testid="project-repo-access-note-empty-roster"
        >
          {projectName} has no other members yet, so only you will be able to
          reach it until you add some.
        </p>
      ) : null}
    </div>
  );
}
