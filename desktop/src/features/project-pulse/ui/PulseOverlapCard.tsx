/**
 * Two missions touching the same paths.
 *
 * Shown to everyone who can read both sides, because an overlap is a fact
 * about shared files rather than a private one about either lane. It is
 * deliberately inert:
 *
 * - **There is no cross-umbrella wake.** Nothing on this card notifies the
 *   other mission, its seats, or its lead. A wake crossing an umbrella
 *   boundary would let one lead schedule another lead's agents, which is a
 *   different authority than reading their work, and this surface has only the
 *   reading one.
 * - **A lead may only note or message the other lead.** That is a human action
 *   taken elsewhere, deliberately, after reading this — not a button here that
 *   quietly does it for them.
 *
 * So this card has no controls at all: no button, no link, no affordance that
 * could imply an action the model cannot honour. Its test pins that, and pins
 * these two sentences, so the next person to reach for a "Notify" button reads
 * the reason first.
 */
import type { PulseMissionOverlap } from "../lib/pulseMissionWire";
import { PulseMissionLineList } from "./PulseMissionRow";

/** Every overlap this read found, rendered as the model worded it. */
export function PulseOverlapCard({
  overlaps,
}: {
  overlaps: readonly PulseMissionOverlap[];
}) {
  if (overlaps.length === 0) return null;
  return (
    <section className="flex flex-col gap-1.5" data-testid="pulse-overlap-card">
      <h2 className="text-sm font-medium text-foreground">Shared paths</h2>
      <ul className="flex flex-col gap-1.5">
        {overlaps.map((overlap) => (
          <li
            className="rounded-md border border-border/60 bg-background/40 p-2"
            data-session-keys={overlap.seats
              .map((seat) => seat.sessionKey)
              .join(",")}
            data-testid="pulse-overlap"
            key={overlap.seats.map((seat) => seat.sha).join(":")}
          >
            <PulseMissionLineList lines={overlap.lines} />
          </li>
        ))}
      </ul>
    </section>
  );
}
