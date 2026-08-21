import type { RelayEvent } from "@/shared/api/types";

type CodingSessionObservedEventListener = (
  events: readonly RelayEvent[],
) => void;

// Mounted session surfaces keep independent authority-scoped stores. This bus
// shares only the signed relay bytes one surface observed; every recipient
// still runs its own channel, signature, and authority classifier before the
// event can enter its store. No event data is retained between mounts or
// communities.
const observedEventListeners = new Set<CodingSessionObservedEventListener>();

export function subscribeToObservedCodingSessionEvents(
  listener: CodingSessionObservedEventListener,
): () => void {
  observedEventListeners.add(listener);
  return () => observedEventListeners.delete(listener);
}

export function fanOutObservedCodingSessionEvents(
  events: readonly RelayEvent[],
  source?: CodingSessionObservedEventListener,
): void {
  if (events.length === 0) return;
  for (const listener of observedEventListeners) {
    if (listener === source) continue;
    try {
      listener(events);
    } catch (error) {
      console.error("Failed to fan out observed coding-session events", error);
    }
  }
}
