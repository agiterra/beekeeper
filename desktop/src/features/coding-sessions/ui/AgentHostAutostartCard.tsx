import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import {
  declineAgentHostAutostart,
  getCodingSessionProviderStatus,
  installAgentHostAutostart,
} from "@/shared/api/tauriSessionProvider";
import { Button } from "@/shared/ui/button";

/**
 * Whether this computer starts its agents at login, as a control that says
 * what it actually does.
 *
 * Two separate facts, kept separate because collapsing them is the whole
 * failure this card exists to prevent: whether the agent host is *registered*
 * to start at login, and whether it is *running now*. A person who quits
 * Beekeeper with neither is told their sessions ended; one with a registration
 * but a dead host is told something different, because the fix is different.
 *
 * The state comes from `status.host.login`, computed in Rust. Nothing here
 * re-derives it from `autostart.installed`, so this card cannot offer to
 * install something that is already installed.
 */
export function AgentHostAutostartCard() {
  const queryClient = useQueryClient();
  const statusQuery = useQuery({
    queryKey: ["coding-session-provider-status"],
    queryFn: getCodingSessionProviderStatus,
    // A browser preview has no host to ask; retrying only delays this card
    // rendering the truth, which is that it does not know.
    retry: false,
  });

  const invalidate = React.useCallback(() => {
    void queryClient.invalidateQueries({
      queryKey: ["coding-session-provider-status"],
    });
  }, [queryClient]);

  const install = useMutation({
    mutationFn: installAgentHostAutostart,
    onSuccess: invalidate,
  });
  const decline = useMutation({
    mutationFn: declineAgentHostAutostart,
    onSuccess: invalidate,
  });
  const busy = install.isPending || decline.isPending;

  const status = statusQuery.data ?? null;
  if (status === null) {
    return (
      <p
        className="text-sm text-muted-foreground"
        data-testid="host-autostart-unknown"
      >
        {statusQuery.isLoading
          ? "Checking whether this computer starts its agents at login…"
          : "This computer could not be asked whether it starts its agents at login."}
      </p>
    );
  }

  const login = status.host.login;
  const warnings = status.host.autostart.warnings ?? [];
  // A refused registration is reported as an error only by its absence, so the
  // request "succeeded" and the state did not change. Say so rather than
  // leaving the button looking like it worked.
  const refusedSilently =
    install.isSuccess && login !== "granted" && warnings.length === 0;

  return (
    <div className="flex flex-col gap-3" data-testid="host-autostart-card">
      {login === "notApplicable" ? (
        <p className="text-sm text-muted-foreground">
          Nothing to start yet. Once you set up coding sessions on this
          computer, Beekeeper can keep them running when the app is closed.
        </p>
      ) : login === "granted" ? (
        <>
          <p className="text-sm" data-testid="host-autostart-granted">
            <span className="font-medium">On.</span> Your agents keep running
            when you quit Beekeeper, and start again when you log in. The menu
            bar icon shows what they are doing.
          </p>
          <div className="flex items-center gap-2">
            <Button
              data-testid="host-autostart-turn-off"
              disabled={busy}
              onClick={() => decline.mutate()}
              size="sm"
              variant="outline"
            >
              Turn off
            </Button>
            <span className="text-2xs text-muted-foreground">
              Turning this off does not end sessions that are running now.
            </span>
          </div>
        </>
      ) : (
        <>
          <p className="text-sm" data-testid="host-autostart-off">
            <span className="font-medium">Off.</span>{" "}
            {login === "declined"
              ? "You chose not to install the background agent host, so your coding sessions end when you quit Beekeeper."
              : "Your coding sessions end when you quit Beekeeper."}
          </p>
          <p className="text-2xs text-muted-foreground">
            Turning this on installs two login items inside Beekeeper: the agent
            host, which runs your sessions, and a menu bar icon that shows them.
            Both live in the app — removing Beekeeper removes them.
          </p>
          <div className="flex items-center gap-2">
            <Button
              data-testid="host-autostart-turn-on"
              disabled={busy}
              onClick={() => install.mutate()}
              size="sm"
            >
              Turn on
            </Button>
          </div>
        </>
      )}

      {warnings.length > 0 ? (
        <ul
          className="flex flex-col gap-1 text-2xs text-destructive"
          data-testid="host-autostart-warnings"
        >
          {warnings.map((warning) => (
            <li key={warning}>{warning}</li>
          ))}
        </ul>
      ) : null}

      {refusedSilently ? (
        <p className="text-2xs text-destructive">
          The registration did not take, and the host did not say why. Check{" "}
          <code>{status.host.autostart.path}</code>.
        </p>
      ) : null}

      {install.error !== null ? (
        <p className="text-2xs text-destructive">{String(install.error)}</p>
      ) : null}
      {decline.error !== null ? (
        <p className="text-2xs text-destructive">{String(decline.error)}</p>
      ) : null}
    </div>
  );
}
