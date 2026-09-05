import { compareSeatsLiveFirst } from "../lib/seatRows";
import type { ProjectAgentsRow, SeatRow } from "../lib/rolesViewModel";
import {
  AGENTS_BY_PROJECT_TITLE,
  PROJECT_SEATS_EMPTY,
  projectSeatCount,
  UNPLACED_TITLE,
} from "./rolesCopy";
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
          className="shrink-0 text-2xs text-muted-foreground"
          data-testid="project-agents-count"
        >
          {projectSeatCount(seats.length)}
        </span>
      </header>
      {seats.length === 0 ? (
        <p
          className="text-xs text-muted-foreground"
          data-testid="project-agents-empty"
        >
          {PROJECT_SEATS_EMPTY}
        </p>
      ) : (
        <ul className="flex flex-col">
          {[...seats].sort(compareSeatsLiveFirst).map((seat) => (
            <li key={seat.key}>
              <SeatRowButton
                columns="project-block"
                onOpen={onOpenSeat}
                seat={seat}
              />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/**
 * Section B: every project in display order with the seats it claims, then
 * the seats nothing claims. A project with no seats is drawn with its empty
 * state — absence is information, not something to tidy away.
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
  return (
    <section
      className="flex flex-col gap-2"
      data-testid="agents-by-project-section"
    >
      <h2 className="text-sm font-medium text-foreground">
        {AGENTS_BY_PROJECT_TITLE}
      </h2>
      <div className="grid gap-2 md:grid-cols-2 xl:grid-cols-3">
        {byProject.map((row) => (
          <ProjectBlock
            key={row.projectId}
            onOpenSeat={onOpenSeat}
            seats={row.seats}
            testId={`project-agents-${row.projectId}`}
            title={row.projectName}
          />
        ))}
        <ProjectBlock
          onOpenSeat={onOpenSeat}
          seats={unplaced}
          testId="project-agents-unplaced"
          title={UNPLACED_TITLE}
        />
      </div>
    </section>
  );
}
