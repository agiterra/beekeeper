import { ExternalLink, MessagesSquare, Plus } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";

import { useAppNavigation } from "@/app/navigation/useAppNavigation";
import type { ChannelCodingSessionIngressEntry } from "@/features/coding-sessions/lib/channelCodingSessionIngress";
import { resolveChannelCodingSessionIngress } from "@/features/coding-sessions/lib/channelCodingSessionIngress";
import { groupCodingSessionCatalog } from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import { openCodingSessionPopout } from "@/features/coding-sessions/lib/codingSessionWindow";
import { useCodingSessionCatalog } from "@/features/coding-sessions/useCodingSessionCatalog";
import {
  codingSessionGoalKey,
  useCodingSessionGoals,
} from "@/features/coding-sessions/useCodingSessionGoals";
import { codingSessionNameKey } from "@/features/coding-sessions/lib/codingSessionName";
import { useCodingSessionNames } from "@/features/coding-sessions/useCodingSessionNames";
import { useIdentityQuery } from "@/shared/api/hooks";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/shared/ui/popover";
import { formatCodingSessionRuntimeLabel } from "@/features/coding-sessions/lib/codingSessionLabels";
import { CodingSessionGoalPill } from "./CodingSessionGoalPill";

type ChannelCodingSessionsMenuProps = {
  channelId: string | null;
  variant?: "inline" | "compact";
};

export function ChannelCodingSessionsMenu({
  channelId,
  variant = "inline",
}: ChannelCodingSessionsMenuProps) {
  const catalog = useCodingSessionCatalog(channelId, null, {
    authorityMode: "open",
  });
  const identity = useIdentityQuery();
  const goalSnapshot = useCodingSessionGoals(channelId ? [channelId] : []);
  const nameSnapshot = useCodingSessionNames(channelId ? [channelId] : []);
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
  const authorityByGeneration = React.useMemo(() => {
    const result = new Map<
      string,
      { founderPubkey: string | null; sessionRef: string | null }
    >();
    for (const umbrella of groupCodingSessionCatalog(
      catalog.entries,
      catalog.creates,
    )) {
      for (const execution of umbrella.executions) {
        for (const generation of [
          ...execution.priorGenerations,
          execution.activeGeneration,
        ]) {
          result.set(generation.generationId, {
            founderPubkey: umbrella.founderPubkey,
            sessionRef: umbrella.sessionRef,
          });
        }
      }
    }
    return result;
  }, [catalog.creates, catalog.entries]);

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
              ? "Open a signed session at its latest generation."
              : "No signed sessions in this channel yet."}
          </p>
        </div>
        <ChannelCodingSessionList
          authorityByGeneration={authorityByGeneration}
          channelId={channelId}
          currentUserPubkey={identity.data?.pubkey ?? null}
          entries={entries}
          goals={goalSnapshot.goals}
          names={nameSnapshot.names}
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
  authorityByGeneration = new Map(),
  channelId = "",
  currentUserPubkey = null,
  entries,
  goals = new Map(),
  names = new Map(),
  onOpen,
  onPopout,
}: {
  authorityByGeneration?: ReadonlyMap<
    string,
    { founderPubkey: string | null; sessionRef: string | null }
  >;
  channelId?: string;
  currentUserPubkey?: string | null;
  entries: ChannelCodingSessionIngressEntry[];
  goals?: ReadonlyMap<
    string,
    import("@/features/coding-sessions/lib/codingSessionGoal").CodingSessionGoal
  >;
  names?: ReadonlyMap<
    string,
    import("@/features/coding-sessions/lib/codingSessionName").CodingSessionName
  >;
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
      {entries.map(({ session, status, executionCount, generationCount }) => {
        const authority = authorityByGeneration.get(session.generationId) ?? {
          founderPubkey: null,
          sessionRef: session.sessionRef,
        };
        const goal =
          authority.sessionRef && authority.founderPubkey
            ? (goals.get(
                codingSessionGoalKey(
                  channelId,
                  authority.sessionRef,
                  authority.founderPubkey,
                ),
              ) ?? null)
            : null;
        const sessionName =
          authority.sessionRef && authority.founderPubkey
            ? (names.get(
                codingSessionNameKey(
                  channelId,
                  authority.sessionRef,
                  authority.founderPubkey,
                ),
              )?.content ?? null)
            : null;
        const note = historyNote(
          session.runtime ?? session.provider,
          executionCount,
          generationCount,
        );
        const providerTitle = session.title.trim();
        const displayName =
          sessionName ??
          (providerTitle && providerTitle !== "Coding session"
            ? providerTitle
            : session.label);
        return (
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
                {displayName}
              </span>
              <span className="shrink-0 text-xs text-muted-foreground">
                {status.label}
              </span>
            </div>
            {note ? (
              <p
                className="mt-0.5 text-2xs text-muted-foreground"
                data-testid="channel-coding-session-history"
              >
                {note}
              </p>
            ) : null}
            <CodingSessionGoalPill
              channelId={channelId}
              currentUserPubkey={currentUserPubkey}
              founderPubkey={authority.founderPubkey}
              goal={goal}
              sessionRef={authority.sessionRef}
              variant="catalog"
            />
            <div className="mt-2 flex items-center justify-end gap-1.5">
              <Button
                aria-label={`Open ${displayName}`}
                data-testid="channel-coding-session-open"
                onClick={() => onOpen(session.generationId)}
                size="sm"
                type="button"
                variant="secondary"
              >
                Open
              </Button>
              <Button
                aria-label={`Pop out ${displayName}`}
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
        );
      })}
    </div>
  );
}

/**
 * What one row stands for beyond the generation it opens.
 *
 * A single-provider row names its agent, which is what the row's own label
 * stopped saying when the display name became the session's name (§2 item 37).
 * A row standing for several providers deliberately names none of them: it
 * opens an umbrella, and picking one to print would be a guess. Generations
 * are disclosed for the same reason — collapsing them into one row (§2 item
 * 38) must not hide that there were several.
 */
function historyNote(
  runtime: string | null,
  executionCount: number,
  generationCount: number,
): string | null {
  const parts: string[] = [];
  if (executionCount > 1) parts.push(`${executionCount} providers`);
  else if (runtime) parts.push(formatCodingSessionRuntimeLabel(runtime));
  if (generationCount > 1) parts.push(`${generationCount} generations`);
  return parts.length === 0 ? null : parts.join(" · ");
}
