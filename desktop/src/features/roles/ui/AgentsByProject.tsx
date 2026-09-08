import type { ProjectAgentsRow, SeatRow } from "../lib/rolesViewModel";
import { compareSeatsLiveFirst } from "../lib/seatRows";
import {
  AGENTS_BY_PROJECT_TITLE,
  projectSeatCount,
  ROLE_SEATS_EMPTY,
  UNPLACED_TITLE,
} from "./rolesCopy";
import { seatStatusDotClass, StatusDot } from "./roleDots";
import { SeatRowButton } from "./SeatRowButton";

function ProjectBlock({
  onOpenSeat,
  seats,
  testId,
  title,
}: {
  onOpenSeat?: (seat: SeatRow) => void;
  seats: readonly SeatRow[];
  testId: string;
  title: string;
}) {
  return (
    <div
      className="flex min-w-0 flex-col gap-1 rounded-lg border border-border bg-card p-3"
      data-testid={testId}
    >
      <header className="flex items-baseline gap-2">
        <h3 className="truncate text-sm font-medium text-foreground">
          {title}
        </h3>
        <span
          className="shrink-0 text-xs tabular-nums text-muted-foreground"
          data-testid="project-agents-count"
        >
          {projectSeatCount(seats.length)}
        </span>
      </header>
      <ul className="flex flex-col gap-0.5">
        {[...seats].sort(compareSeatsLiveFirst).map((seat) => (
          <li className="flex min-w-0 items-center gap-1.5" key={seat.key}>
            <StatusDot
              className={seatStatusDotClass(seat.status)}
              title={seat.status}
            />
            <SeatRowButton
              columns="project-block"
              onOpen={onOpenSeat}
              seat={seat}
            />
          </li>
        ))}
      </ul>
    </div>
  );
}

/**
 * Section B: the open sessions, grouped by the project that claims them, then
 * the ones nothing claims.
 *
 * Only groups that have a session are drawn. A project with none is not news
 * — repeating "no agents in open sessions" once per project was the page's
 * loudest empty state and said nothing the one line below does not. When
 * nothing anywhere holds a session, that single line is the whole section, so
 * absence is still stated exactly once.
 */
export function AgentsByProject({
  byProject,
  unplaced,
  onOpenSeat,
}: {
  byProject: readonly ProjectAgentsRow[];
  unplaced: readonly SeatRow[];
  onOpenSeat?: (seat: SeatRow) => void;
}) {
  const filled = byProject.filter((row) => row.seats.length > 0);
  return (
    <section
      className="flex flex-col gap-2"
      data-testid="agents-by-project-section"
    >
      <h2 className="text-sm font-medium text-foreground">
        {AGENTS_BY_PROJECT_TITLE}
      </h2>
      {filled.length === 0 && unplaced.length === 0 ? (
        <p
          className="text-xs text-muted-foreground"
          data-testid="project-agents-empty"
        >
          {ROLE_SEATS_EMPTY}
        </p>
      ) : (
        <div className="grid gap-2 md:grid-cols-2 xl:grid-cols-3">
          {filled.map((row) => (
            <ProjectBlock
              key={row.projectId}
              onOpenSeat={onOpenSeat}
              seats={row.seats}
              testId={`project-agents-${row.projectId}`}
              title={row.projectName}
            />
          ))}
          {unplaced.length > 0 ? (
            <ProjectBlock
              onOpenSeat={onOpenSeat}
              seats={unplaced}
              testId="project-agents-unplaced"
              title={UNPLACED_TITLE}
            />
          ) : null}
        </div>
      )}
    </section>
  );
}
