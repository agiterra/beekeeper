/**
 * The React half of the observer seam.
 *
 * Lane W3 renders screens; lane W2 owns the relay reader. Rather than have
 * the routes import a hook that does not exist yet, the reader registers
 * itself once at module load:
 *
 * ```ts
 * // lane W2, at the bottom of its hook module
 * bindCodingSessionObserverSource(useRelayCodingSessionObserver);
 * ```
 *
 * and the app imports that module once (e.g. from `main.tsx` or the reader's
 * own barrel). Until something binds, every screen renders the honest
 * "reader not wired" state rather than an empty list that looks like "no
 * sessions" — a blank list would be a lie about the relay.
 *
 * The binding must happen before the first render and must not change
 * afterwards: the bound source is called as a hook, so its identity has to be
 * stable for the life of the page.
 */
import {
  type CodingSessionObserverSource,
  type CodingSessionObserverView,
  emptyCodingSessionObserverView,
} from "./observer-contract.ts";

const UNBOUND_ERROR =
  "Coding-session reader is not wired up in this build — no relay subscription is running.";

let boundSource: CodingSessionObserverSource | null = null;

/** Register the relay-backed reader. Call once, before the first render. */
export function bindCodingSessionObserverSource(
  source: CodingSessionObserverSource,
): void {
  boundSource = source;
}

/** Drop the binding. Tests only. */
export function resetCodingSessionObserverSource(): void {
  boundSource = null;
}

function unboundSource(channelId: string | null): CodingSessionObserverView {
  return emptyCodingSessionObserverView({
    channelId,
    connection: "closed",
    lastError: UNBOUND_ERROR,
  });
}

/**
 * Read the observer view for one channel.
 *
 * `channelId === null` means the repo has no session channel; the view comes
 * back idle and the screens explain that rather than showing an empty list.
 */
export function useCodingSessionObserver(
  channelId: string | null,
): CodingSessionObserverView {
  const source = boundSource ?? unboundSource;
  return source(channelId);
}
