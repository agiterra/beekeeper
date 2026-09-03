/**
 * Pulse's disk row: how many worktrees this machine holds, how much of them is
 * rebuildable, and which of them hold work nobody has committed.
 *
 * This component renders strings and re-words nothing. Every sentence it shows
 * was composed by `pulseDiskRow.ts` from rows the host classified in Rust; if
 * a number looks wrong, it is wrong there, which is where the tests are.
 *
 * Mounting is `ProjectPulseView.tsx`, which belongs to Lane L9. Until that
 * mount line lands this component ships with its tests and no caller, and
 * `bee sessions worktree status --all` carries the same truth meanwhile.
 */
import type { PulseDiskRow as PulseDiskRowModel } from "@/features/project-pulse/lib/pulseDiskRow";

export function PulseDiskRow({ row }: { row: PulseDiskRowModel }) {
  return (
    <section className="mt-3" data-testid="pulse-disk-row">
      <p className="text-sm text-foreground">{row.line}</p>
      {row.held.length > 0 ? (
        <ul className="mt-1 space-y-0.5" data-testid="pulse-disk-held">
          {row.held.map((entry) => (
            <li
              className="text-2xs text-muted-foreground"
              key={entry.key}
              title={entry.path}
            >
              <span className="font-mono">{entry.sessionRef.slice(0, 8)}</span>{" "}
              {entry.line}
            </li>
          ))}
        </ul>
      ) : null}
      {row.unrecorded > 0 ? (
        <p
          className="mt-1 text-2xs text-muted-foreground"
          data-testid="pulse-disk-unrecorded"
        >
          {row.unrecorded} worktree{row.unrecorded === 1 ? "" : "s"} were not
          recorded by this host, so nothing here removes them.
        </p>
      ) : null}
    </section>
  );
}
