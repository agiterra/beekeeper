import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import { useAgentProgressCoordination } from "@/features/agent-progress/lib/agentProgressCoordination";
import {
  CODING_SESSION_CAPACITY_MAX,
  CODING_SESSION_CAPACITY_UNLIMITED,
  codingSessionCapacityChoice,
  codingSessionCapacityLabel,
  codingSessionCapacityPending,
  parseCodingSessionCapacityInput,
} from "@/features/coding-sessions/lib/codingSessionCapacity";
import { useChannelsQuery } from "@/features/channels/hooks";
import { isSessionTransportChannel } from "@/shared/api/channelTypes";
import {
  getCodingSessionCapacity,
  getCodingSessionProviderStatus,
  setCodingSessionCapacity,
} from "@/shared/api/tauriSessionProvider";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";

/**
 * How many agent processes this computer will hold at once.
 *
 * The cap existed before this panel did, as a compiled-in 4 that surfaced only
 * as a refusal — read, reasonably, as a limit from the model vendor ("there
 * can only be 4 concurrent Claude sessions?", 2026-08-24). It is Bee Keeper's
 * own, it is about this machine's capacity, and it is now the person's to set.
 *
 * Everything here is stated rather than implied: what is running now, what the
 * ceiling is, and — when they differ — that a saved change reaches the provider
 * only at its next start.
 */
export function CodingSessionCapacityCard() {
  const queryClient = useQueryClient();
  const settingsQuery = useQuery({
    queryKey: ["coding-session-capacity"],
    queryFn: getCodingSessionCapacity,
  });
  const statusQuery = useQuery({
    queryKey: ["coding-session-provider-status"],
    queryFn: getCodingSessionProviderStatus,
  });

  const settings = settingsQuery.data ?? null;
  const providerPubkey = statusQuery.data?.providerPubkey ?? null;
  const runningNow = useRunningSessionCount(providerPubkey);

  const [draft, setDraft] = React.useState<number>(4);
  const [dirty, setDirty] = React.useState(false);
  // Until the person edits, the field mirrors what is stored; after, it is
  // theirs. Without the guard a refetch would overwrite half-typed input.
  const storedLimit =
    settings === null
      ? null
      : (settings.maxSessions ?? settings.defaultMaxSessions);
  React.useEffect(() => {
    if (dirty || storedLimit === null) return;
    if (storedLimit !== CODING_SESSION_CAPACITY_UNLIMITED)
      setDraft(storedLimit);
  }, [dirty, storedLimit]);

  const save = useMutation({
    mutationFn: (maxSessions: number | null) =>
      setCodingSessionCapacity(maxSessions),
    onSuccess: (next) => {
      queryClient.setQueryData(["coding-session-capacity"], next);
      setDirty(false);
    },
  });

  const choice =
    settings === null
      ? { kind: "default" as const }
      : codingSessionCapacityChoice(settings.maxSessions);
  const pending =
    settings === null
      ? null
      : codingSessionCapacityPending({
          maxSessions: settings.maxSessions,
          defaultMaxSessions: settings.defaultMaxSessions,
          runningMaxSessions: settings.runningMaxSessions,
          providerRunning: statusQuery.data?.running === true,
        });

  return (
    <div
      className="flex flex-col gap-4 px-4 py-4"
      data-testid="settings-coding-session-capacity"
    >
      <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
        <p className="text-sm" data-testid="coding-session-running-count">
          <span className="font-medium">
            {runningNow.count} running now
            {runningNow.complete ? "" : " (at least)"}
          </span>
        </p>
        <p className="text-xs text-muted-foreground">
          {settings === null
            ? "Reading this computer's limit…"
            : `Limit: ${codingSessionCapacityLabel(
                settings.maxSessions,
                settings.defaultMaxSessions,
              )}.`}{" "}
          A session holds its slot until it is stopped or goes idle for long
          enough to be reclaimed.
        </p>
      </div>

      <div className="flex flex-wrap items-end gap-3">
        <div className="flex flex-col gap-1.5">
          <label
            className="text-xs font-medium text-muted-foreground"
            htmlFor="coding-session-capacity-limit"
          >
            Maximum at once
          </label>
          <Input
            className="h-9 w-24"
            data-testid="coding-session-capacity-input"
            disabled={choice.kind === "unlimited" || settingsQuery.isPending}
            id="coding-session-capacity-limit"
            inputMode="numeric"
            max={CODING_SESSION_CAPACITY_MAX}
            min={1}
            onChange={(event) => {
              setDirty(true);
              setDraft((previous) =>
                parseCodingSessionCapacityInput(event.target.value, previous),
              );
            }}
            type="number"
            value={choice.kind === "unlimited" ? "" : draft}
          />
        </div>
        <Button
          data-testid="coding-session-capacity-save"
          disabled={choice.kind === "unlimited" || save.isPending}
          onClick={() => save.mutate(draft)}
          size="sm"
          type="button"
        >
          {save.isPending ? "Saving…" : "Save"}
        </Button>
        <Button
          data-testid="coding-session-capacity-unlimited"
          disabled={save.isPending}
          onClick={() =>
            save.mutate(
              choice.kind === "unlimited"
                ? draft
                : CODING_SESSION_CAPACITY_UNLIMITED,
            )
          }
          size="sm"
          type="button"
          variant="outline"
        >
          {choice.kind === "unlimited" ? "Set a limit" : "Unlimited"}
        </Button>
      </div>

      {choice.kind === "unlimited" ? (
        <p
          className="text-xs text-muted-foreground"
          data-testid="coding-session-capacity-unlimited-note"
        >
          No ceiling. Each live session is an agent process on this computer —
          its memory, its CPU, and its provider's own usage limits still apply.
        </p>
      ) : null}
      {pending === null ? null : (
        <p
          className="rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs"
          data-testid="coding-session-capacity-pending"
        >
          {pending}
        </p>
      )}
      {save.isError ? (
        <p
          className="text-xs text-destructive"
          data-testid="coding-session-capacity-error"
        >
          {save.error instanceof Error
            ? save.error.message
            : "Could not save the limit."}
        </p>
      ) : null}
    </div>
  );
}

/**
 * How many of this provider's executions are answering right now.
 *
 * Counted from the signed kind-24223 leases the coordination fold already
 * reads (§2 item 36), not from a number the app keeps for itself: a lease is
 * the only evidence in this system that an agent process is alive. The count
 * is scoped to *this* computer's provider, because that is whose ceiling this
 * panel sets, and it is disclosed as a floor when the read was partial — a
 * session in a channel this client cannot see is still holding a slot.
 */
function useRunningSessionCount(providerPubkey: string | null): {
  count: number;
  complete: boolean;
} {
  const channelsQuery = useChannelsQuery({ includeSessionTransports: true });
  const channelIds = React.useMemo(
    () =>
      (channelsQuery.data ?? [])
        .filter(
          (channel) => channel.isMember || isSessionTransportChannel(channel),
        )
        .map((channel) => channel.id)
        .sort(),
    [channelsQuery.data],
  );
  const channelsUnresolved =
    channelsQuery.isPending ||
    channelsQuery.isError ||
    channelsQuery.isFetching;
  const { read } = useAgentProgressCoordination(channelIds, channelsUnresolved);

  return React.useMemo(() => {
    if (read === null || providerPubkey === null) {
      return { count: 0, complete: false };
    }
    const live = new Set<string>();
    for (const session of read.sessions) {
      for (const generation of session.generations) {
        if (
          generation.reachability === "provider_reachable" &&
          generation.providerAuthorityPubkey.toLowerCase() ===
            providerPubkey.toLowerCase()
        ) {
          live.add(generation.executionKey);
        }
      }
    }
    return { count: live.size, complete: read.complete && !channelsUnresolved };
  }, [channelsUnresolved, providerPubkey, read]);
}
