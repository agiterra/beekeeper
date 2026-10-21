/**
 * Planning through the relay's plugin route. Every read and every write is
 * an operation of a plugin that implements `beekeeper.planning/1` — the
 * first-party one or any other registered the same way. This module holds no
 * planning rules: it asks the plugin, signs what it drafts as the user
 * (after checking it), and reports exactly what landed.
 *
 * The UI module is compiled into the app and lazy-loaded with the Planning
 * route; it cannot be installed at runtime the way the relay-side component
 * can (`docs/plugins.md` § Limits).
 */
import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { relayClient } from "@/shared/api/relayClient";
import { RelayRejectedEventError } from "@/shared/api/relayRejection";
import { signRelayEvent } from "@/shared/api/tauri";
import {
  type PluginInfo,
  PluginCallError,
  invokeRelayPlugin,
  listRelayPlugins,
  providersOf,
} from "@/shared/plugins/pluginClient";
import {
  type DraftRun,
  DraftUnconfirmedError,
  PluginPartialWriteError,
  executePluginDrafts,
} from "@/shared/plugins/pluginDrafts";

import { PlanningPartialWriteError } from "./planningErrors";
import type {
  PlanningCapabilities,
  PlanningCreateResult,
  PlanningIssue,
  PlanningMutations,
  PlanningRead,
  PlanningState,
} from "./planningTypes";

const PUBLISH_TIMEOUT = "Timed out saving the change.";

/** One refetch per burst of live events. */
const LIVE_REFETCH_DEBOUNCE_MS = 750;

/** The operation API the Planning surface speaks. */
export const PLANNING_API = "beekeeper.planning/1";

/** One history entry, as the plugin's `activity` operation returns it. */
export type ActivityEntry = {
  id: string;
  actor: string;
  createdAt: number;
  /** What changed, without the actor: "set status to In Progress". */
  summary: string;
  /** The person an assignment names. */
  subject?: string | null;
};

export type PlanningProviders =
  | { kind: "loading" }
  | { kind: "ready"; providers: PluginInfo[] }
  | { kind: "unavailable"; reason: string }
  | { kind: "error"; message: string };

const PROVIDERS_KEY = ["relay-plugins"] as const;

/** Which plugins on this relay implement Planning. */
export function usePlanningProviders(): PlanningProviders {
  const query = useQuery({
    queryKey: PROVIDERS_KEY,
    queryFn: listRelayPlugins,
    staleTime: 5 * 60_000,
    retry: (count, error) =>
      !(error instanceof PluginCallError && error.status < 500) && count < 2,
  });
  if (query.isPending) return { kind: "loading" };
  if (query.isError) {
    const error = query.error;
    if (error instanceof PluginCallError && error.code === "route-not-found") {
      return {
        kind: "unavailable",
        reason:
          "This community's relay predates plugins, so it runs no Planning provider.",
      };
    }
    return { kind: "error", message: error.message };
  }
  const providers = providersOf(query.data, PLANNING_API);
  if (providers.length === 0) {
    return {
      kind: "unavailable",
      reason:
        "This community's relay runs no Planning provider. An operator registers one in the relay's plugin registry.",
    };
  }
  return { kind: "ready", providers };
}

function planningKey(plugin: string | null, coordinate: string | null) {
  return ["planning", plugin ?? "none", coordinate ?? "none"] as const;
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export type PluginPlanning = {
  state: PlanningState;
  /** What the relay decided this viewer may do here, once it has said. */
  relayAccess: "read" | "write" | null;
  capabilities: PlanningCapabilities | null;
  mutations: PlanningMutations;
};

/**
 * Planning for one project through `plugin`. `null` for either means there
 * is nothing to read yet; `unavailable` is passed through as the state.
 */
export function usePluginPlanning(
  coordinate: string | null,
  provider: PluginInfo | null,
  unavailable: string | null,
): PluginPlanning {
  const queryClient = useQueryClient();
  const plugin = provider?.name ?? null;
  const base = React.useMemo(
    () => planningKey(plugin, coordinate),
    [coordinate, plugin],
  );
  const enabled = plugin !== null && coordinate !== null;

  const capabilitiesQuery = useQuery({
    queryKey: [...base, "capabilities"],
    queryFn: async () =>
      (
        await invokeRelayPlugin<PlanningCapabilities>(
          plugin as string,
          "capabilities",
          coordinate as string,
          {},
        )
      ).result,
    enabled,
    staleTime: 10 * 60_000,
  });
  const readQuery = useQuery({
    queryKey: [...base, "read"],
    queryFn: async () => {
      const reply = await invokeRelayPlugin<Omit<PlanningRead, "provider">>(
        plugin as string,
        "read",
        coordinate as string,
        {},
      );
      return {
        read: { ...reply.result, provider: reply.plugin.name } as PlanningRead,
        access: reply.access,
      };
    },
    enabled,
    refetchInterval: 60_000,
  });

  // Follow the records live: whatever kinds the plugin says it reads, in
  // this project. The read itself always comes from the plugin.
  const readKinds = provider?.readKinds ?? [];
  const kindsKey = readKinds.join(",");
  React.useEffect(() => {
    if (!enabled || kindsKey === "") return;
    let disposed = false;
    let unsubscribe: (() => Promise<void>) | null = null;
    let timer: ReturnType<typeof setTimeout> | null = null;
    void relayClient
      .subscribeLive(
        {
          kinds: kindsKey.split(",").map(Number),
          "#a": [coordinate as string],
          since: Math.floor(Date.now() / 1_000) - 900,
          limit: 100,
        },
        () => {
          // A burst of events is one refetch: each read re-folds the
          // project's history, so per-event invalidation would rescan it
          // once per op. Capabilities do not change with records.
          if (timer !== null) return;
          timer = setTimeout(() => {
            timer = null;
            void queryClient.invalidateQueries({
              predicate: (query) =>
                query.queryKey.length > base.length &&
                base.every((part, i) => query.queryKey[i] === part) &&
                query.queryKey[base.length] !== "capabilities",
            });
          }, LIVE_REFETCH_DEBOUNCE_MS);
        },
      )
      .then((handle) => {
        if (!handle) return;
        if (disposed) void handle();
        else unsubscribe = handle;
      })
      .catch(() => {
        // The 60 s poll is the fallback; no read result is invented here.
      });
    return () => {
      disposed = true;
      if (timer !== null) clearTimeout(timer);
      if (unsubscribe) void unsubscribe();
    };
  }, [base, coordinate, enabled, kindsKey, queryClient]);

  const state: PlanningState = React.useMemo(() => {
    if (unavailable !== null)
      return { kind: "unavailable", reason: unavailable };
    const read = readQuery.data?.read ?? null;
    if (readQuery.isError) {
      const error = readQuery.error;
      if (
        error instanceof PluginCallError &&
        error.code === "scope-forbidden"
      ) {
        return {
          kind: "unavailable",
          reason:
            "You are not on this project's roster, so its issues are not shown to you.",
        };
      }
      return { kind: "error", read, message: message(error) };
    }
    if (capabilitiesQuery.isError && !read) {
      return {
        kind: "error",
        read: null,
        message: message(capabilitiesQuery.error),
      };
    }
    if (!read || !capabilitiesQuery.data) return { kind: "loading" };
    return { kind: "ready", read, refreshing: readQuery.isFetching };
  }, [
    capabilitiesQuery.data,
    capabilitiesQuery.error,
    capabilitiesQuery.isError,
    readQuery.data,
    readQuery.error,
    readQuery.isError,
    readQuery.isFetching,
    unavailable,
  ]);

  const mutations = React.useMemo<PlanningMutations>(() => {
    const write = async (
      operation: string,
      input: unknown,
    ): Promise<{ result: unknown; run: DraftRun }> => {
      if (plugin === null || coordinate === null) {
        throw new Error("This project has no Planning provider to write to.");
      }
      const reply = await invokeRelayPlugin<unknown>(
        plugin,
        operation,
        coordinate,
        input,
      );
      try {
        const run = await executePluginDrafts(reply.drafts, coordinate, {
          sign: signRelayEvent,
          publish: async (event) => {
            try {
              return await relayClient.publishEvent(
                event,
                PUBLISH_TIMEOUT,
                "The relay refused the change.",
              );
            } catch (error) {
              // Only an explicit `OK false` is a refusal. A timeout, or a
              // socket that closed or stalled with the event in flight, may
              // have landed: never report that as refused.
              if (error instanceof RelayRejectedEventError) throw error;
              throw new DraftUnconfirmedError(
                `The relay did not confirm the change (${message(error)}).`,
              );
            }
          },
        });
        return { result: reply.result, run };
      } catch (error) {
        if (error instanceof PluginPartialWriteError) {
          throw new PlanningPartialWriteError(error.message);
        }
        throw error;
      } finally {
        void queryClient.invalidateQueries({ queryKey: base });
      }
    };
    const target = (issue: PlanningIssue) => ({
      containerId: issue.containerId,
      issueId: issue.issueId,
    });
    const strict = async (operation: string, input: unknown) => {
      const { run } = await write(operation, input);
      if (run.unconfirmed) {
        throw new PlanningPartialWriteError(run.unconfirmed.message);
      }
      // A single-group write either lands or throws; a failed group here
      // would be a provider that drafted `skip-group` for a one-field edit.
      if (run.failed.length > 0) {
        throw new PlanningPartialWriteError(
          run.failed.map((f) => `${f.group}: ${f.message}`).join("; "),
        );
      }
    };
    return {
      createIssue: async (input) => {
        const { result, run } = await write("create-issue", input);
        const key = (result as { key?: unknown }).key;
        if (typeof key !== "string") {
          throw new Error(
            "The Planning provider did not name the issue it created.",
          );
        }
        // The issue's key is the provider's answer before anything was
        // published, so an uncertain create still closes the draft: a retry
        // could mint a second issue, and the next read will show the first.
        const failed: PlanningCreateResult["failed"] = [
          ...run.failed.map((f) => ({ field: f.group, message: f.message })),
          ...(run.unconfirmed
            ? [
                {
                  field: run.unconfirmed.group,
                  message: run.unconfirmed.message,
                },
              ]
            : []),
          ...run.notAttempted.map((group) => ({
            field: group,
            message:
              "not attempted, because an earlier change was not confirmed",
          })),
        ];
        return { key, failed };
      },
      setTitle: (issue, title) =>
        strict("set-title", { ...target(issue), title }),
      setDescription: (issue, description) =>
        strict("set-description", { ...target(issue), description }),
      setStatus: (issue, statusId) =>
        strict("set-status", { ...target(issue), statusId }),
      setPriority: (issue, priorityId) =>
        strict("set-priority", { ...target(issue), priorityId }),
      setAssignee: (issue, assignee) =>
        strict("set-assignee", { ...target(issue), assignee }),
    };
  }, [base, coordinate, plugin, queryClient]);

  return {
    state,
    relayAccess: readQuery.data?.access ?? null,
    capabilities: capabilitiesQuery.data ?? null,
    mutations,
  };
}

export type ActivityState =
  | { kind: "loading" }
  | { kind: "ready"; entries: ActivityEntry[] }
  | { kind: "error"; message: string };

/** One issue's history from the plugin's `activity` operation. */
export function usePlanningActivity(
  coordinate: string | null,
  plugin: string | null,
  issue: PlanningIssue | null,
): ActivityState {
  const query = useQuery({
    queryKey: [
      ...planningKey(plugin, coordinate),
      "activity",
      issue?.key ?? "none",
      issue?.updatedAt ?? 0,
    ],
    queryFn: async () =>
      (
        await invokeRelayPlugin<{ entries: ActivityEntry[] }>(
          plugin as string,
          "activity",
          coordinate as string,
          { containerId: issue?.containerId, issueId: issue?.issueId },
        )
      ).result.entries,
    enabled: plugin !== null && coordinate !== null && issue !== null,
  });
  if (query.isError) return { kind: "error", message: message(query.error) };
  if (!query.data) return { kind: "loading" };
  return { kind: "ready", entries: query.data };
}
