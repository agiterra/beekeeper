import * as React from "react";

import { BACKGROUND_UPDATE_CHECK_INTERVAL_MS } from "./use-updater";
import {
  relayBuildDrift,
  type RelayBuildDrift,
} from "@/features/settings/relayBuildDrift";
import { fetchRelayBuildIdentity } from "@/shared/api/communityProfile";
import { getAppBuildIdentity } from "@/shared/api/appBuild";

const UNKNOWN: RelayBuildDrift = {
  state: "unknown",
  method: "none",
  reason: "relay-commit-unknown",
};

/**
 * This app's own build identity, fetched once per process.
 *
 * It is three compile-time constants behind an IPC hop, so it cannot change
 * while the app runs and there is no reason to ask twice — or to re-ask on a
 * community switch.
 */
let appIdentityPromise: ReturnType<typeof getAppBuildIdentity> | null = null;
function appIdentityOnce() {
  appIdentityPromise ??= getAppBuildIdentity();
  return appIdentityPromise;
}

/** Test seam: drop the memoized app identity between cases. */
export function resetAppBuildIdentityCache() {
  appIdentityPromise = null;
}

/**
 * Whether this app is behind the relay at `relayUrl`.
 *
 * Re-checks on the same cadence as the updater, so the two build-currency
 * facts refresh together rather than contradicting each other for hours.
 * Keyed on the relay URL: switching community re-asks, and a verdict from the
 * previous relay is never carried across — it would name the wrong relay.
 *
 * Every failure resolves to an `unknown` verdict rather than throwing. Not
 * knowing is a disclosed state here with its own sentence in Settings, not an
 * error worth surfacing.
 */
export function useRelayBuildDrift(relayUrl: string | null): RelayBuildDrift {
  const [drift, setDrift] = React.useState<RelayBuildDrift>(UNKNOWN);

  React.useEffect(() => {
    if (!relayUrl) {
      setDrift(UNKNOWN);
      return;
    }
    let cancelled = false;

    const check = async () => {
      try {
        const [app, relay] = await Promise.all([
          appIdentityOnce(),
          fetchRelayBuildIdentity(relayUrl),
        ]);
        if (cancelled) return;
        setDrift(
          relayBuildDrift({
            app: {
              commit: app.commit,
              commitCount: app.commitCount,
              sourceDirty: app.sourceDirty,
            },
            relay: {
              commit: relay.commit,
              commitCount: relay.commitCount,
              software: relay.software,
            },
          }),
        );
      } catch {
        if (!cancelled) setDrift(UNKNOWN);
      }
    };

    void check();
    const timer = window.setInterval(
      () => void check(),
      BACKGROUND_UPDATE_CHECK_INTERVAL_MS,
    );
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [relayUrl]);

  return drift;
}
