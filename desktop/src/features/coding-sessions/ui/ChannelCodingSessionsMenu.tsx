import { ExternalLink, MessagesSquare, Plus } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";

import { useAppNavigation } from "@/app/navigation/useAppNavigation";
import type { ChannelCodingSessionIngressEntry } from "@/features/coding-sessions/lib/channelCodingSessionIngress";
import { resolveChannelCodingSessionIngress } from "@/features/coding-sessions/lib/channelCodingSessionIngress";
import { openCodingSessionPopout } from "@/features/coding-sessions/lib/codingSessionWindow";
import { useCodingSessionCatalog } from "@/features/coding-sessions/useCodingSessionCatalog";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/shared/ui/popover";

type ChannelCodingSessionsMenuProps = {
  channelId: string | null;
  variant?: "inline" | "compact";
};

export function ChannelCodingSessionsMenu({
  channelId,
  variant = "inline",
}: ChannelCodingSessionsMenuProps) {
  const catalog = useCodingSessionCatalog(channelId);
  const entries = React.useMemo(
    () =>
      resolveChannelCodingSessionIngress({
        activeChannelId: channelId,
        catalog,
      }),
    [catalog, channelId],
  );
  const { goCodingSession, goNewCodingSession } = useAppNavigation();
  const [open, setOpen] = React.useState(false);

  const handleOpen = React.useCallback(
    (generationId: string) => {
      if (!channelId) return;
      setOpen(false);
      void goCodingSession(channelId, generationId);
    },
    [channelId, goCodingSession],
  );
  const handleCreate = React.useCallback(() => {
    if (!channelId) return;
    setOpen(false);
    void goNewCodingSession(channelId);
  }, [channelId, goNewCodingSession]);
  const handlePopout = React.useCallback(
    (generationId: string) => {
      if (!channelId) return;
      setOpen(false);
      void openCodingSessionPopout(channelId, generationId).catch((error) => {
        toast.error(
          error instanceof Error
            ? error.message
            : "Unable to open the coding-session window.",
        );
      });
    },
    [channelId],
  );

  // The donor hid the trigger entirely when a channel had no sessions, which
  // works when a project shelf is the discovery surface. Standalone sessions
  // have no other entry point, so the trigger always renders and the empty
  // state is what invites the first one.
  if (!channelId) {
    return null;
  }

  return (
    <Popover onOpenChange={setOpen} open={open}>
      <PopoverTrigger asChild>
        <ChannelCodingSessionsTrigger
          count={entries.length}
          variant={variant}
        />
      </PopoverTrigger>
      <PopoverContent
        align="end"
        className="w-80 p-2"
        data-testid="channel-coding-sessions-menu"
        sideOffset={8}
      >
        <div className="px-2 pt-1 pb-2">
          <p className="text-sm font-semibold">Coding sessions</p>
          <p className="text-xs text-muted-foreground">
            {entries.length > 0
              ? "Open an exact signed session generation."
              : "No signed sessions in this channel yet."}
          </p>
        </div>
        <ChannelCodingSessionList
          entries={entries}
          onOpen={handleOpen}
          onPopout={handlePopout}
        />
        <Button
          className="mt-1 w-full justify-start"
          data-testid="channel-coding-sessions-new"
          onClick={handleCreate}
          size="sm"
          type="button"
          variant="ghost"
        >
          <Plus />
          New coding session
        </Button>
      </PopoverContent>
    </Popover>
  );
}

/**
 * The Sessions doorway.
 *
 * A `forwardRef` because `PopoverTrigger asChild` merges its open/close props
 * and its ref onto this element — a plain function component would swallow
 * both, and the popover would never open.
 */
type ChannelCodingSessionsTriggerProps = Omit<
  React.ComponentPropsWithoutRef<typeof Button>,
  "variant"
> & {
  count: number;
  variant: "inline" | "compact";
};

export const ChannelCodingSessionsTrigger = React.forwardRef<
  HTMLButtonElement,
  ChannelCodingSessionsTriggerProps
>(function ChannelCodingSessionsTrigger({ count, variant, ...rest }, ref) {
  const compact = variant === "compact";
  return (
    <Button
      aria-label={`Coding sessions (${count})`}
      className={compact ? undefined : "h-8 gap-1.5 px-2.5"}
      data-testid="channel-coding-sessions-trigger"
      ref={ref}
      size={compact ? "icon" : undefined}
      title={compact ? `Coding sessions (${count})` : undefined}
      type="button"
      variant="outline"
      {...rest}
    >
      <MessagesSquare />
      {compact ? null : (
        <>
          <span className="text-sm font-medium">Sessions</span>
          <span className="min-w-[1ch] text-xs tabular-nums text-muted-foreground">
            {count}
          </span>
        </>
      )}
    </Button>
  );
});

export function ChannelCodingSessionList({
  entries,
  onOpen,
  onPopout,
}: {
  entries: ChannelCodingSessionIngressEntry[];
  onOpen: (generationId: string) => void;
  onPopout: (generationId: string) => void;
}) {
  if (entries.length === 0) {
    return (
      <p
        className="px-2 pb-2 text-xs text-muted-foreground"
        data-testid="channel-coding-sessions-empty"
      >
        Sessions started here appear in this list as soon as the provider signs
        its first event.
      </p>
    );
  }

  return (
    <div className="flex max-h-80 flex-col gap-1 overflow-y-auto">
      {entries.map(({ session, status }) => (
        <div
          className="rounded-lg border border-border/60 bg-background/60 p-2.5"
          data-generation-id={session.generationId}
          data-testid="channel-coding-session-entry"
          key={session.generationId}
        >
          <div className="flex min-w-0 items-center gap-2">
            <span
              aria-hidden
              className={cn(
                "h-2 w-2 shrink-0 rounded-full",
                status.kind === "working"
                  ? "bg-emerald-500"
                  : status.kind === "idle"
                    ? "bg-muted-foreground/50"
                    : "bg-amber-500",
              )}
            />
            <span className="min-w-0 flex-1 truncate text-sm font-medium">
              {session.label}
            </span>
            <span className="shrink-0 text-xs text-muted-foreground">
              {status.label}
            </span>
          </div>
          <div className="mt-2 flex items-center justify-end gap-1.5">
            <Button
              aria-label={`Open ${session.label}`}
              data-testid="channel-coding-session-open"
              onClick={() => onOpen(session.generationId)}
              size="sm"
              type="button"
              variant="secondary"
            >
              Open
            </Button>
            <Button
              aria-label={`Pop out ${session.label}`}
              data-testid="channel-coding-session-popout"
              onClick={() => onPopout(session.generationId)}
              size="icon-xs"
              title="Pop out"
              type="button"
              variant="ghost"
            >
              <ExternalLink />
            </Button>
          </div>
        </div>
      ))}
    </div>
  );
}
