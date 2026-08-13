import * as React from "react";

import {
  getSessionConsent,
  grantConsent,
  isSessionConsented,
  revokeConsent,
  setSessionConsent,
  subscribeSessionConsent,
} from "../lib/sessionConsent";

/**
 * Subscribe to per-window shell interaction consent. Backed by an external
 * store so the Settings toggle, the session screen, and the just-in-time
 * prompt stay in lockstep. Buzz writes to a session only when `isConsented`
 * is true.
 */
export function useSessionConsent(): {
  isConsented: (workspaceId: string) => boolean;
  grant: (workspaceId: string) => void;
  revoke: (workspaceId: string) => void;
} {
  const consent = React.useSyncExternalStore(
    subscribeSessionConsent,
    getSessionConsent,
    getSessionConsent,
  );

  const isConsented = React.useCallback(
    (workspaceId: string) => isSessionConsented(consent, workspaceId),
    [consent],
  );
  const grant = React.useCallback((workspaceId: string) => {
    setSessionConsent(grantConsent(getSessionConsent(), workspaceId));
  }, []);
  const revoke = React.useCallback((workspaceId: string) => {
    setSessionConsent(revokeConsent(getSessionConsent(), workspaceId));
  }, []);

  return { isConsented, grant, revoke };
}
