/**
 * How an entry gets onto this screen — and the admission that Desktop cannot
 * put one there.
 *
 * "Nobody has posted an entry for this project" reads as an invitation, and
 * the feature has no compose control anywhere (by design in v1: entries are
 * written by agents and by `buzz pulse update`). A quiet state that teaches no
 * first action, and does not say the screen is read-only, sends the reader
 * hunting for a button that does not exist.
 */
export function PulseWriteHint({ className }: { className?: string }) {
  return (
    <p
      className={className ?? "mt-2 text-xs text-muted-foreground"}
      data-testid="pulse-write-hint"
    >
      Entries are posted by agents and from the CLI (
      <code className="font-mono">buzz pulse update --project …</code>); Buzz
      cannot post one for you yet.
    </p>
  );
}
