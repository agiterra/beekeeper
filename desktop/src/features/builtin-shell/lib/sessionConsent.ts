// Per-window interaction consent for built-in shell sessions.
//
// SAFETY MODEL: buzz never writes to a shell session (types text, sends keys,
// or replies to a blocked prompt) unless the owner has explicitly consented to
// that specific session. Consent is default-OFF, per-session (keyed by
// workspaceId), and revocable. It is granted either just-in-time (the first
// interaction attempt prompts) or ahead of time via a Settings toggle.
//
// This is a genuine access control: reading a session (screen text) is NOT
// gated — that's the owner viewing their own machine; only *writing* into a
// session requires consent.
//
// Machine-scoped (localStorage), so intentionally NOT in resetCommunityState.
// Consent lives only on this device; it is never published to the relay.

const STORAGE_KEY = "buzz.shell-sessions.consent.v1";

export type SessionConsent = {
  /** Workspace ids the owner has allowed buzz to interact with. */
  consented: string[];
};

const EMPTY: SessionConsent = { consented: [] };

/** Whether buzz may write to (interact with) this session. */
export function isSessionConsented(
  consent: SessionConsent,
  workspaceId: string,
): boolean {
  return consent.consented.includes(workspaceId);
}

export function grantConsent(
  consent: SessionConsent,
  workspaceId: string,
): SessionConsent {
  if (consent.consented.includes(workspaceId)) return consent;
  return { consented: [...consent.consented, workspaceId] };
}

export function revokeConsent(
  consent: SessionConsent,
  workspaceId: string,
): SessionConsent {
  if (!consent.consented.includes(workspaceId)) return consent;
  return {
    consented: consent.consented.filter((id) => id !== workspaceId),
  };
}

function isConsent(value: unknown): value is SessionConsent {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  return (
    Array.isArray(v.consented) &&
    v.consented.every((x) => typeof x === "string")
  );
}

function load(): SessionConsent {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return EMPTY;
    const parsed: unknown = JSON.parse(raw);
    return isConsent(parsed) ? parsed : EMPTY;
  } catch {
    return EMPTY;
  }
}

function persist(consent: SessionConsent): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(consent));
  } catch {
    // Ignore unavailable/full storage; consent still applies this session.
  }
}

// External store so the settings toggle, the session screen, and the just-in
// -time prompt all observe the same consent state.
let current: SessionConsent | null = null;
const listeners = new Set<() => void>();

export function getSessionConsent(): SessionConsent {
  if (current === null) current = load();
  return current;
}

export function setSessionConsent(next: SessionConsent): void {
  current = next;
  persist(next);
  for (const listener of listeners) listener();
}

export function subscribeSessionConsent(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
