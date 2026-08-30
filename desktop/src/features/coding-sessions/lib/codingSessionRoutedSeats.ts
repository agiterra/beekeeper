/**
 * The routed seats in one umbrella, as lines a surface can render.
 *
 * Read straight off the 44223 each provider signed — never re-derived — so
 * the sentence the operator sees is the decision that was actually published
 * with the seat. A seat nothing routed produces no line at all: "not routed"
 * and "routed to the default" are different facts, and only one of them ever
 * happened.
 */
import { describeCodingSessionRouting } from "./codingSessionRouting";
import type { CodingSessionExecution } from "./codingSessionTypes";

/** One routed seat's line, keyed by its execution. */
export type CodingSessionRoutedSeat = {
  key: string;
  /** `routed: <class>/<tier> → <provider>/<model> (<effort>) — <reason>`. */
  line: string;
};

/** Every routed seat in this umbrella, in the umbrella's own order. */
export function listCodingSessionRoutedSeats(
  executions: readonly CodingSessionExecution[],
): CodingSessionRoutedSeat[] {
  return executions.flatMap((execution) => {
    const routing = execution.activeGeneration.routing;
    return routing === null
      ? []
      : [
          {
            key: execution.executionKey,
            line: describeCodingSessionRouting(routing),
          },
        ];
  });
}
