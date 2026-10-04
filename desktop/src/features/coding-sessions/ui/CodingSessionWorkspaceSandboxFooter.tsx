import {
  type CodingSessionComposerSandbox,
  CodingSessionComposerSandboxChip,
} from "./CodingSessionComposerSandboxChip";

/**
 * The sandbox chip for a session that has no composer dock (SV-17).
 *
 * Boundary rows leave the transcript for the composer chip (decision D3). A
 * closed session, or one with no command target, mounts no composer, so
 * without this row a reader reviewing it would lose the fact that it ran with
 * full access or with its boundary not enforced. The chip is the same one the
 * composer shows, read-only: it records the boundary rather than changing it.
 */
export function CodingSessionWorkspaceSandboxFooter({
  sandbox,
  sessionClosed,
}: {
  sandbox: CodingSessionComposerSandbox;
  sessionClosed: boolean;
}) {
  return (
    <div
      className="flex h-10 shrink-0 items-center gap-2 border-t border-border/50 px-4 text-xs text-muted-foreground"
      data-testid="coding-session-sandbox-footer"
    >
      <CodingSessionComposerSandboxChip
        readOnlyNote={
          sessionClosed
            ? "This session is closed. This is the boundary its agent last reported; there is nothing here to change."
            : "This session has no command target, so its boundary cannot be changed from here. This is what its agent last reported."
        }
        sandbox={sandbox}
      />
      {/* One row, always: the jump pill is positioned to clear it. */}
      <span className="min-w-0 truncate">
        {sessionClosed ? "Session closed" : "No command target published"}
      </span>
    </div>
  );
}
