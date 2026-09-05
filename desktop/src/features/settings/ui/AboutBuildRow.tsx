import * as React from "react";

import {
  aboutBuildState,
  aboutBuildTooltip,
  shortSha,
  UNKNOWN,
} from "./aboutBuildRowCopy";
import { SettingsOptionRow } from "./SettingsOptionGroup";
import {
  getAppBuildIdentity,
  type AppBuildIdentity,
} from "@/shared/api/appBuild";
import {
  fetchRelayBuildIdentity,
  type RelayBuildIdentity,
} from "@/shared/api/communityProfile";
import { useCommunities } from "@/features/communities/useCommunities";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/shared/ui/tooltip";

/**
 * One row that answers "what am I running": this app's commit beside the
 * relay's, and what the pair means.
 *
 * The two facts already existed — `get_app_build_identity` (three compile-time
 * constants) and the relay's NIP-11 `software_commit` — but only the *verdict*
 * was ever rendered, in a sentence. The shas themselves were unreachable from
 * the app, so checking a refresh meant an ssh or a `curl`, which is exactly
 * what the build stamp was added to stop (PLAN-2026-09-05 §1-B).
 *
 * Every value is a disclosed non-answer or a real one: an absent commit reads
 * `unknown` and never an empty string, a dash, or a stale previous value. The
 * relay's `unknown` names its cause, because hive answers `unknown` on every
 * build for one specific reason — its deployer predates the `BUZZ_SOURCE_SHA`
 * fix (2ba548c9e) — and a bare "unknown" would send the reader to the wrong
 * place.
 */
export function AboutBuildRow() {
  const { activeCommunity } = useCommunities();
  const relayUrl = activeCommunity?.relayUrl ?? null;
  const [app, setApp] = React.useState<AppBuildIdentity | null>(null);
  const [relay, setRelay] = React.useState<RelayBuildIdentity | null>(null);

  React.useEffect(() => {
    let cancelled = false;
    // Failure resolves to `null`, which renders `unknown`. Not knowing is a
    // disclosed state on this row, not an error worth a retry banner.
    void getAppBuildIdentity()
      .then((identity) => {
        if (!cancelled) setApp(identity);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  React.useEffect(() => {
    if (!relayUrl) {
      setRelay(null);
      return;
    }
    let cancelled = false;
    // Keyed on the relay URL: switching community re-asks, and a verdict from
    // the previous relay is never carried across — it would name the wrong one.
    setRelay(null);
    void fetchRelayBuildIdentity(relayUrl)
      .then((identity) => {
        if (!cancelled) setRelay(identity);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [relayUrl]);

  const input = {
    app: {
      commit: app?.commit ?? null,
      commitCount: app?.commitCount ?? null,
      sourceDirty: app?.sourceDirty ?? null,
    },
    relay: {
      commit: relay?.commit ?? null,
      commitCount: relay?.commitCount ?? null,
      software: relay?.software ?? null,
    },
  };
  const { label } = aboutBuildState(input);
  const tooltip = aboutBuildTooltip(input, relay?.buildTime ?? null);

  return (
    <SettingsOptionRow data-testid="about-build-row">
      <Tooltip>
        <TooltipTrigger asChild>
          <div className="min-w-0 cursor-default">
            <p className="text-sm font-normal">
              <span data-testid="about-build-app">
                App {shortSha(app?.commit)}
              </span>
              <span className="text-muted-foreground/70"> · </span>
              <span data-testid="about-build-relay">
                relay {shortSha(relay?.commit)}
              </span>
            </p>
            <p
              className="text-xs font-normal text-muted-foreground/70"
              data-settings-subcopy
              data-testid="about-build-state"
            >
              {label}
            </p>
          </div>
        </TooltipTrigger>
        <TooltipContent
          align="start"
          className="whitespace-pre font-mono text-2xs"
        >
          {tooltip}
        </TooltipContent>
      </Tooltip>
    </SettingsOptionRow>
  );
}

/** Re-exported so a caller can render the same literal this row does. */
export { UNKNOWN as ABOUT_BUILD_UNKNOWN };
