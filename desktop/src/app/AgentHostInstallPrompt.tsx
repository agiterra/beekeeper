import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import {
  declineAgentHostAutostart,
  getCodingSessionProviderStatus,
  installAgentHostAutostart,
} from "@/shared/api/tauriSessionProvider";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { isMacPlatform } from "@/shared/lib/platform";

/**
 * Asks, once, whether this computer should run its agents in the background.
 *
 * Registering something that starts at every login, forever, is a change to
 * the machine and not to a document, so it is a question rather than
 * something the app helps itself to. It also happens to be the only moment
 * anything tells a person the agent host exists — before this, the fact that
 * quitting Beekeeper ended their coding sessions was discoverable only by
 * losing work to it.
 *
 * **When it opens.** Only on `status.host.login === "shouldAsk"`, which is
 * Rust's answer and means: an identity is commissioned here, nothing is
 * registered, and nobody has said no. A refusal is recorded on the machine, so
 * declining is final until the person revisits it in Settings → Coding
 * sessions; this component never re-proposes it and there is no counter of
 * times asked to get wrong.
 *
 * On a headless machine there is no window and nobody to ask — `bee host
 * install` is the whole story there, which is why this lives in the app and
 * not in the host.
 */
export function AgentHostInstallPrompt() {
  // Windows and Linux have no tray today and the host's login registration is
  // not wired for them, so there is nothing honest to offer.
  if (!isMacPlatform()) return null;
  return <MacAgentHostInstallPrompt />;
}

function MacAgentHostInstallPrompt() {
  const queryClient = useQueryClient();
  const statusQuery = useQuery({
    queryKey: ["coding-session-provider-status"],
    queryFn: getCodingSessionProviderStatus,
    retry: false,
  });

  /**
   * Whether this window has already put the question. A person who closes the
   * dialog without answering is not asked again in this session — the next
   * launch asks, because nothing was recorded.
   *
   * Deliberately not persisted: a stored "asked once" would be a third piece
   * of state to keep in step with the registration and the refusal, and the
   * one it would prevent is the mildest of the failures here.
   */
  const [dismissed, setDismissed] = React.useState(false);

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

  const login = statusQuery.data?.host.login ?? null;
  const open = login === "shouldAsk" && !dismissed;
  const busy = install.isPending || decline.isPending;

  // A registration that could not be written comes back as warnings on a
  // successful call, not as a rejection. Keep the dialog up and show them —
  // closing over a failure is how somebody comes to believe their agents
  // survive a reboot.
  const warnings = statusQuery.data?.host.autostart.warnings ?? [];
  const failed =
    install.isSuccess && login === "shouldAsk" && warnings.length > 0;

  return (
    <Dialog
      onOpenChange={(next) => {
        if (!next && !busy) setDismissed(true);
      }}
      open={open || failed}
    >
      <DialogContent data-testid="agent-host-install-prompt">
        <DialogHeader>
          <DialogTitle>Keep your agents running?</DialogTitle>
          <DialogDescription>
            Right now your coding sessions end when you quit Beekeeper. This
            computer can run them in the background instead, so they keep
            working with the app closed and start again when you log in.
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-2 text-sm">
          <p>Turning this on installs two login items:</p>
          <ul className="flex list-disc flex-col gap-1 pl-5 text-muted-foreground">
            <li>
              the <span className="font-medium">agent host</span>, which runs
              your sessions
            </li>
            <li>
              a <span className="font-medium">menu bar icon</span>, which shows
              what they are doing
            </li>
          </ul>
          <p className="text-2xs text-muted-foreground">
            Both live inside Beekeeper, so removing the app removes them. You
            can change this any time in Settings → Coding sessions.
          </p>
        </div>
        {warnings.length > 0 ? (
          <ul
            className="flex flex-col gap-1 text-2xs text-destructive"
            data-testid="agent-host-install-prompt-warnings"
          >
            {warnings.map((warning) => (
              <li key={warning}>{warning}</li>
            ))}
          </ul>
        ) : null}
        {install.error !== null ? (
          <p className="text-2xs text-destructive">{String(install.error)}</p>
        ) : null}
        <DialogFooter>
          <Button
            data-testid="agent-host-install-prompt-decline"
            disabled={busy}
            onClick={() => decline.mutate()}
            variant="outline"
          >
            Don&rsquo;t install
          </Button>
          <Button
            data-testid="agent-host-install-prompt-install"
            disabled={busy}
            onClick={() => install.mutate()}
          >
            Install
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
