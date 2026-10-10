import * as React from "react";
import {
  ArrowLeft,
  ArrowRight,
  ExternalLink,
  MoreHorizontal,
  PictureInPicture2,
  RotateCw,
  Server,
  X,
} from "lucide-react";

import type {
  SessionPreviewServer,
  SessionPreviewState,
} from "@/shared/api/tauriSessionPreview";
import { Button } from "@/shared/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";

import { sessionPreviewDisplayUrl } from "../lib/previewModel";
import { sessionPreviewCurrentServer } from "../lib/previewTabModel";

export type SessionPreviewToolbarActions = {
  onBack: () => void;
  onForward: () => void;
  onReload: () => void;
  onNavigate: (raw: string) => void;
  onPopout: () => void;
  onToggleFloat: () => void;
  onClose: () => void;
};

/** This computer's local servers, for the switcher (ledger 371(i)). */
export type SessionPreviewToolbarServers = {
  servers: readonly SessionPreviewServer[];
  loading: boolean;
  error: string | null;
  /** The menu opened or closed: the list is read only while it is open. */
  onOpenChange: (open: boolean) => void;
  /** Navigate the open page to this server. */
  onChoose: (server: SessionPreviewServer) => void;
};

/**
 * Back, forward, reload, the server switcher, the URL field, pop out and the
 * overflow (float or dock, close). A refused URL is shown under the field in the broker's own
 * sentence (`preview_url_refused`), never swapped for a system-browser
 * handoff.
 */
export function SessionPreviewToolbar({
  actions,
  floating,
  refusal,
  servers,
  state,
}: {
  actions: SessionPreviewToolbarActions;
  floating: boolean;
  refusal: string | null;
  servers: SessionPreviewToolbarServers;
  state: SessionPreviewState;
}) {
  const current = sessionPreviewCurrentServer(servers.servers, state.url);
  const shown = state.url ? sessionPreviewDisplayUrl(state.url) : "";
  const [draft, setDraft] = React.useState(shown);
  const [editing, setEditing] = React.useState(false);
  React.useEffect(() => {
    if (!editing) setDraft(shown);
  }, [editing, shown]);

  return (
    <div className="shrink-0 border-b border-border/60">
      <div
        className="flex h-9 items-center gap-0.5 px-1.5"
        data-testid="session-preview-toolbar"
      >
        <Button
          aria-label="Back"
          data-testid="session-preview-back"
          disabled={!state.canGoBack}
          onClick={actions.onBack}
          size="icon-xs"
          variant="ghost"
        >
          <ArrowLeft />
        </Button>
        <Button
          aria-label="Forward"
          data-testid="session-preview-forward"
          disabled={!state.canGoForward}
          onClick={actions.onForward}
          size="icon-xs"
          variant="ghost"
        >
          <ArrowRight />
        </Button>
        <Button
          aria-label="Reload"
          data-testid="session-preview-reload"
          onClick={actions.onReload}
          size="icon-xs"
          variant="ghost"
        >
          <RotateCw />
        </Button>
        <DropdownMenu onOpenChange={servers.onOpenChange}>
          <DropdownMenuTrigger asChild>
            <Button
              aria-label="Switch to another local server"
              data-testid="session-preview-servers-switch"
              size="icon-xs"
              variant="ghost"
            >
              <Server />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start">
            <DropdownMenuLabel className="text-2xs font-medium uppercase tracking-wide text-muted-foreground">
              Local servers
            </DropdownMenuLabel>
            {servers.servers.map((server) => (
              <DropdownMenuItem
                data-current={server === current ? "true" : undefined}
                data-testid="session-preview-servers-switch-item"
                key={server.url}
                onSelect={() => servers.onChoose(server)}
              >
                <span className="flex min-w-0 flex-col">
                  <span className="truncate text-xs">
                    {sessionPreviewDisplayUrl(server.url)}
                    {server === current ? " · open now" : ""}
                  </span>
                  {server.process ? (
                    <span className="truncate text-2xs text-muted-foreground">
                      {server.process}
                    </span>
                  ) : null}
                </span>
              </DropdownMenuItem>
            ))}
            {servers.servers.length === 0 ? (
              <DropdownMenuItem
                data-testid="session-preview-servers-switch-empty"
                disabled
              >
                {servers.error ??
                  (servers.loading
                    ? "Looking for servers on this computer…"
                    : "No local server is listening on this computer.")}
              </DropdownMenuItem>
            ) : null}
          </DropdownMenuContent>
        </DropdownMenu>
        <form
          className="mx-1 min-w-0 flex-1"
          onSubmit={(event) => {
            event.preventDefault();
            setEditing(false);
            if (draft.trim()) actions.onNavigate(draft);
          }}
        >
          <input
            aria-invalid={refusal ? true : undefined}
            aria-label="Page address"
            className="h-6 w-full rounded-md border border-input/40 bg-muted/40 px-2 text-xs text-foreground outline-hidden focus-visible:ring-1 focus-visible:ring-ring"
            data-testid="session-preview-url"
            onBlur={() => setEditing(false)}
            onChange={(event) => setDraft(event.target.value)}
            onFocus={(event) => {
              setEditing(true);
              event.target.select();
            }}
            placeholder="localhost:5173"
            spellCheck={false}
            value={draft}
          />
        </form>
        <Button
          aria-label="Pop out to its own window"
          data-testid="session-preview-popout"
          onClick={actions.onPopout}
          size="icon-xs"
          variant="ghost"
        >
          <ExternalLink />
        </Button>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              aria-label="More"
              data-testid="session-preview-overflow"
              size="icon-xs"
              variant="ghost"
            >
              <MoreHorizontal />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem
              data-testid="session-preview-toggle-float"
              onSelect={actions.onToggleFloat}
            >
              <PictureInPicture2 aria-hidden className="size-4" />
              {floating ? "Dock in the panel" : "Float over the transcript"}
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              data-testid="session-preview-close"
              onSelect={actions.onClose}
            >
              <X aria-hidden className="size-4" />
              Close preview
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      {refusal ? (
        <p
          className="px-2.5 pb-1.5 text-2xs text-destructive"
          data-testid="session-preview-refusal"
          role="alert"
        >
          {refusal}
        </p>
      ) : null}
    </div>
  );
}
