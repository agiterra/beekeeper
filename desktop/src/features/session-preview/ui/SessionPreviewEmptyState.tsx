import { Clock, Server } from "lucide-react";

import type { SessionPreviewServer } from "@/shared/api/tauriSessionPreview";
import { cn } from "@/shared/lib/cn";

import {
  type SessionPreviewBinding,
  SESSION_PREVIEW_CHOOSE_DRIVER,
  sessionPreviewDisplayUrl,
} from "../lib/previewModel";
import type { SessionPreviewRecent } from "../lib/previewRecents";

function Row({
  disabled,
  label,
  onOpen,
  sub,
  testId,
}: {
  disabled: boolean;
  label: string;
  onOpen: () => void;
  sub: string | null;
  testId: string;
}) {
  return (
    <button
      className={cn(
        "flex w-full flex-col items-start rounded-md px-2.5 py-1.5 text-left transition-colors hover:bg-accent/60",
        disabled && "cursor-not-allowed opacity-50 hover:bg-transparent",
      )}
      data-testid={testId}
      disabled={disabled}
      onClick={onOpen}
      type="button"
    >
      <span className="truncate text-sm text-foreground">{label}</span>
      {sub ? (
        <span className="truncate text-2xs text-muted-foreground">{sub}</span>
      ) : null}
    </button>
  );
}

/**
 * What the Browser shows before a page is open: this computer's live local
 * servers (polled while shown), then "Recently used". When the session has
 * several agents and none is focused, the person first chooses which one may
 * drive the preview, and nothing opens until they do.
 */
export function SessionPreviewEmptyState({
  binding,
  closedNote,
  onChoose,
  onOpen,
  recents,
  servers,
  serversError,
  serversLoading,
}: {
  binding: SessionPreviewBinding;
  closedNote: string | null;
  onChoose: (executionKey: string) => void;
  onOpen: (url: string, title: string | null) => void;
  recents: readonly SessionPreviewRecent[];
  servers: readonly SessionPreviewServer[];
  serversError: string | null;
  serversLoading: boolean;
}) {
  const blocked = binding.kind === "choose";
  return (
    <div
      className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-2 py-3"
      data-testid="session-preview-empty"
    >
      {closedNote ? (
        <p className="px-2.5 text-xs text-muted-foreground">{closedNote}</p>
      ) : null}
      {binding.kind === "choose" ? (
        <section
          className="rounded-lg border border-border/60 bg-muted/30 px-1 py-2"
          data-testid="session-preview-choose-driver"
        >
          <h3 className="px-2.5 pb-1 text-xs font-medium text-foreground">
            {SESSION_PREVIEW_CHOOSE_DRIVER}
          </h3>
          {binding.options.map((option) => (
            <Row
              disabled={false}
              key={option.executionKey}
              label={option.label}
              onOpen={() => onChoose(option.executionKey)}
              sub={null}
              testId="session-preview-choose-driver-option"
            />
          ))}
        </section>
      ) : null}
      <section data-testid="session-preview-servers">
        <h3 className="flex items-center gap-1.5 px-2.5 pb-1 text-2xs font-medium uppercase tracking-wide text-muted-foreground">
          <Server aria-hidden className="size-3" />
          Local servers
        </h3>
        {servers.map((server) => (
          <Row
            disabled={blocked}
            key={server.url}
            label={server.title ?? sessionPreviewDisplayUrl(server.url)}
            onOpen={() => onOpen(server.url, server.title)}
            sub={[sessionPreviewDisplayUrl(server.url), server.process]
              .filter(Boolean)
              .join(" · ")}
            testId="session-preview-server"
          />
        ))}
        {servers.length === 0 ? (
          <p className="px-2.5 text-xs text-muted-foreground">
            {serversError ??
              (serversLoading
                ? "Looking for servers on this computer…"
                : "No local server is listening on this computer.")}
          </p>
        ) : null}
      </section>
      <section data-testid="session-preview-recents">
        <h3 className="flex items-center gap-1.5 px-2.5 pb-1 text-2xs font-medium uppercase tracking-wide text-muted-foreground">
          <Clock aria-hidden className="size-3" />
          Recently used
        </h3>
        {recents.map((recent) => (
          <Row
            disabled={blocked}
            key={recent.url}
            label={recent.title ?? sessionPreviewDisplayUrl(recent.url)}
            onOpen={() => onOpen(recent.url, recent.title)}
            sub={recent.title ? sessionPreviewDisplayUrl(recent.url) : null}
            testId="session-preview-recent"
          />
        ))}
        {recents.length === 0 ? (
          <p className="px-2.5 text-xs text-muted-foreground">
            Nothing opened on this computer yet.
          </p>
        ) : null}
      </section>
    </div>
  );
}
