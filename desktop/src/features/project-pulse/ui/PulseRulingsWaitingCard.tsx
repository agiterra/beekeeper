/**
 * What needs you: the rulings this reader holds, then everyone else's.
 *
 * Two orderings are deliberate. The reader's own queue leads, because a
 * decision waiting on *you* is the one fact on this screen that only you can
 * clear. And the second heading lists the **rest** of the open rulings, not
 * all of them: counting a ruling twice under a heading that implies somebody
 * else holds it is a coordination lie about who is blocking whom.
 *
 * The unknown-viewer case is the one this card exists to get right. When
 * `viewerPubkey` is null this client does not know who is reading, so it does
 * not know what waits on them. That is not zero. It renders no count, no empty
 * list, and no "0" — only the sentence the model supplied for the state, and
 * if the model supplied none, nothing at all. Inventing "0 waiting on you"
 * from an unknown identity is exactly the comfortable guess this surface is
 * built to refuse.
 *
 * A ruling carries no `lines[]` of its own on the wire, so this card renders
 * the `question` the model supplied and the ids that identify the ruling —
 * never a sentence composed here. The full waiting sentence ("Waiting on the
 * founder · asked by … · 42m ago: …") is a mission line, and the mission row
 * is where it renders.
 */
import type {
  PulseMissionRowsResponse,
  PulseMissionRuling,
} from "../lib/pulseMissionWire";
import { pulseMissionNotReadLines } from "../lib/pulseMissionWire";
import { PulseMissionLineList } from "./PulseMissionRow";

function RulingRow({
  ruling,
  testId,
}: {
  ruling: PulseMissionRuling;
  testId: string;
}) {
  return (
    <li
      className="rounded-md border border-border/60 bg-background/40 p-2"
      data-asked-at={ruling.askedAt ?? ""}
      data-asked-by={ruling.askedBy}
      data-held-on={ruling.heldOn}
      data-request-id={ruling.requestId}
      data-session-key={ruling.sessionKey}
      data-testid={testId}
    >
      {ruling.question ? (
        <p
          className="text-sm text-foreground"
          data-testid="pulse-ruling-question"
        >
          {ruling.question}
        </p>
      ) : null}
      <p
        className="truncate font-mono text-2xs text-muted-foreground"
        data-testid="pulse-ruling-request"
      >
        {ruling.requestId}
      </p>
    </li>
  );
}

function Heading({
  children,
  testId,
}: {
  children: React.ReactNode;
  testId: string;
}) {
  return (
    <h2 className="text-sm font-medium text-foreground" data-testid={testId}>
      {children}
    </h2>
  );
}

/** The decision queue: what waits on the reader, then what waits on others. */
export function PulseRulingsWaitingCard({
  rows,
}: {
  rows: PulseMissionRowsResponse;
}) {
  const viewerKnown = rows.viewerPubkey !== null;
  const waiting = viewerKnown ? rows.rulingsWaitingOnViewer : [];
  const waitingIds = new Set(waiting.map((ruling) => ruling.requestId));
  const others = rows.openRulings.filter(
    (ruling) => !waitingIds.has(ruling.requestId),
  );
  const notRead = viewerKnown ? [] : pulseMissionNotReadLines(rows);
  if (waiting.length === 0 && others.length === 0 && notRead.length === 0) {
    return null;
  }
  return (
    <section
      className="flex flex-col gap-2"
      data-testid="pulse-rulings-card"
      data-viewer-known={viewerKnown ? "yes" : "no"}
    >
      {waiting.length > 0 ? (
        <div className="flex flex-col gap-1.5">
          <Heading testId="pulse-rulings-waiting-heading">
            Waiting on you
          </Heading>
          <ul
            className="flex flex-col gap-1.5"
            data-testid="pulse-rulings-waiting"
          >
            {waiting.map((ruling) => (
              <RulingRow
                key={ruling.requestId}
                ruling={ruling}
                testId="pulse-ruling-waiting"
              />
            ))}
          </ul>
        </div>
      ) : null}

      {notRead.length > 0 ? (
        <PulseMissionLineList
          className="flex flex-col gap-0.5"
          lines={notRead}
          testId="pulse-rulings-viewer-unknown"
        />
      ) : null}

      {others.length > 0 ? (
        <div className="flex flex-col gap-1.5">
          <Heading testId="pulse-rulings-open-heading">Open rulings</Heading>
          <ul
            className="flex flex-col gap-1.5"
            data-testid="pulse-rulings-open"
          >
            {others.map((ruling) => (
              <RulingRow
                key={ruling.requestId}
                ruling={ruling}
                testId="pulse-ruling-open"
              />
            ))}
          </ul>
        </div>
      ) : null}
    </section>
  );
}
