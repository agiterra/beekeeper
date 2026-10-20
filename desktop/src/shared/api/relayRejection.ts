/**
 * The relay answered an EVENT with `OK false`: it saw the event and refused
 * it, so the event did not land. Every other publish failure — a timeout, a
 * socket that closed or stalled with the event in flight — leaves the event's
 * fate unknown, and a caller that retries such a failure can mint a
 * duplicate. Code that must tell the two apart checks for this type, never
 * for message text.
 */
export class RelayRejectedEventError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "RelayRejectedEventError";
  }
}
