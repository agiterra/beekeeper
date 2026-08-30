import * as React from "react";
import { useQuery } from "@tanstack/react-query";

import { useChannelsQuery } from "@/features/channels/hooks";
import { useManagedAgentsQuery } from "@/features/agents/hooks";
import { isSessionTransportChannel } from "@/shared/api/channelTypes";
import { useIdentityQuery } from "@/shared/api/hooks";
import { getCodingSessionWorkdirState } from "@/shared/api/tauriCodingSessionWorkdirs";
import {
  getCodingSessionProviderModels,
  getCodingSessionProviderRuntimes,
  getCodingSessionProviderStatus,
} from "@/shared/api/tauriSessionProvider";
import { useStableArrayShallow } from "@/shared/hooks/useStableReference";
import {
  useCodingSessionHire,
  type UseCodingSessionHireInput,
} from "../hooks/useCodingSessionHire";
import { useRolePacksProject } from "@/features/agents/ui/useRolePacksProject";
import { readModelRegistry } from "../lib/codingSessionRegistrySource";
import { useGlobalCodingSessionCatalog } from "../useCodingSessionCatalog";
import { groupCodingSessionCatalog } from "../lib/codingSessionUmbrellaModel";

/**
 * The one place `session.hire` is answered.
 *
 * Renders nothing. It exists because {@link useCodingSessionHire} has to be
 * mounted somewhere for a running desktop to honour a hire at all, and the
 * only honest place is the app shell: a hire arrives whether or not anybody
 * has a session screen open, and a host that only answered while a particular
 * view was mounted would leave a lead waiting out its whole turn budget for a
 * computer that was running the entire time.
 *
 * Mounted inside the community-scoped subtree, so switching communities
 * remounts it and it never answers a hire with the previous relay's catalog.
 * It holds no module-level state of its own — the answered-set lives in the
 * hook's ref and dies with the remount — so there is nothing for
 * `resetCommunityState()` to clear.
 *
 * Sourcing lives here; the answering lives in {@link CodingSessionHireRunner},
 * which takes everything as ordinary input so a test can mount the real hook
 * against injected effects.
 */
export function CodingSessionHireHost() {
  const identityQuery = useIdentityQuery();
  const channelsQuery = useChannelsQuery({ includeSessionTransports: true });
  const managedAgents = useManagedAgentsQuery();
  const providerStatus = useQuery({
    queryKey: ["coding-session-provider-status"],
    queryFn: getCodingSessionProviderStatus,
  });
  const runtimes = useQuery({
    queryKey: ["coding-session-provider-runtimes"],
    queryFn: getCodingSessionProviderRuntimes,
  });
  const workdirs = useQuery({
    queryKey: ["coding-session-workdir-state"],
    queryFn: getCodingSessionWorkdirState,
  });
  // What each ready runtime actually offers, straight from the provider —
  // `list_runtimes()` publishes a hardcoded `["default"]` for every row
  // (`session_provider/runtimes.rs:269`), so the runtime table can say which
  // runtimes exist but never which models they run. This is the same command
  // the create dialog's picker is built from, so a hire and a hand-made
  // session are judged against one list.
  const readyInstanceRefs = useStableArrayShallow(
    React.useMemo(
      () =>
        (runtimes.data ?? [])
          .filter((runtime) => runtime.authState === "ready")
          .map((runtime) => runtime.instanceRef)
          .sort(),
      [runtimes.data],
    ),
  );
  const modelCatalogsQuery = useQuery({
    queryKey: ["coding-session-provider-model-catalogs", readyInstanceRefs],
    queryFn: async () => {
      const entries = await Promise.all(
        readyInstanceRefs.map(async (instanceRef) => {
          // One adapter that will not answer must not cost the others their
          // catalog — and a runtime with no catalog refuses no model at all.
          const models = await getCodingSessionProviderModels(
            instanceRef,
          ).catch(() => null);
          return models === null
            ? null
            : ([instanceRef, models.allowedModels] as const);
        }),
      );
      return entries.filter((entry) => entry !== null);
    },
    enabled: readyInstanceRefs.length > 0,
  });
  const modelCatalogs = React.useMemo(
    () => new Map<string, readonly string[]>(modelCatalogsQuery.data ?? []),
    [modelCatalogsQuery.data],
  );

  const channelIds = useStableArrayShallow(
    React.useMemo(
      () =>
        (channelsQuery.data ?? [])
          // Session transports are readable through the project ACL without a
          // channel_members row, so a founder reads them too.
          .filter(
            (channel) => channel.isMember || isSessionTransportChannel(channel),
          )
          .map((channel) => channel.id)
          .sort(),
      [channelsQuery.data],
    ),
  );
  const catalog = useGlobalCodingSessionCatalog(channelIds, {
    authorityMode: "open",
  });
  const umbrellas = React.useMemo(
    () =>
      groupCodingSessionCatalog(
        catalog.entries.map((entry) => entry.session),
        catalog.creates,
      ),
    [catalog.entries, catalog.creates],
  );

  const workdirState = workdirs.data ?? null;
  // The shared model registry, or the honest reason this host has none.
  //
  // The router needs `team/model-registry.yaml` out of the project checkout,
  // and the project it belongs to is the one the Agents tab already resolves
  // (`features/agents/lib/rolePacksProject.ts`) — the same answer the role-pack
  // installer and the registry badge use, so a routed hire and a badge can
  // never be talking about two different checkouts. `readModelRegistry` reads
  // it when a reader is installed and answers `unreadable`, naming the path,
  // when one is not. Either way nothing is ever routed against a registry copy
  // compiled into the app, which nobody could check against the file the team
  // edits.
  const rolePacksProject = useRolePacksProject();
  const projectRef = rolePacksProject.project?.address ?? null;
  const registryQuery = useQuery({
    queryKey: ["coding-session-model-registry", projectRef],
    queryFn: () => readModelRegistry(projectRef),
  });
  const registry = React.useMemo(
    () =>
      registryQuery.data ?? {
        kind: "unreadable" as const,
        why: "this host has not finished reading the model registry yet.",
      },
    [registryQuery.data],
  );
  const checkoutForChannel = React.useCallback(
    (channelId: string) => {
      if (workdirState === null) return null;
      // The channel's own directory first, then the most recent one this
      // computer used. Never a guess beyond that: with nothing remembered the
      // seat is published without a worktree rather than cut from whatever
      // happens to be first on disk.
      return (
        workdirState.byChannel[channelId]?.path ??
        workdirState.mru[0]?.path ??
        null
      );
    },
    [workdirState],
  );

  const targetForActor = React.useCallback(
    (channelId: string, actorPubkey: string) => {
      const actor = actorPubkey.trim().toLowerCase();
      let best: {
        target: NonNullable<
          (typeof catalog.entries)[number]["session"]["commandTarget"]
        >;
        at: string;
      } | null = null;
      for (const entry of catalog.entries) {
        if (entry.channelId !== channelId) continue;
        const record = entry.session;
        if (record.agentRef?.trim().toLowerCase() !== actor) continue;
        if (record.commandTarget === null) continue;
        // The newest generation of that seat: an older one is a target the
        // provider has already retired, and a turn addressed to it is a turn
        // nobody reads.
        if (best === null || record.lastEventAt > best.at) {
          best = { target: record.commandTarget, at: record.lastEventAt };
        }
      }
      return best?.target ?? null;
    },
    [catalog.entries],
  );

  return (
    <CodingSessionHireRunner
      agents={managedAgents.data ?? []}
      channelIds={channelIds}
      checkoutForChannel={checkoutForChannel}
      modelCatalogs={modelCatalogs}
      operatorPubkey={identityQuery.data?.pubkey ?? null}
      registry={registry}
      providerAuthorityPubkey={providerStatus.data?.providerPubkey ?? null}
      runtimes={runtimes.data ?? []}
      targetForActor={targetForActor}
      umbrellas={umbrellas}
    />
  );
}

/**
 * The hook, mounted. Nothing else.
 *
 * Separate from the sourcing above so the thing under test is the thing that
 * runs: a test mounts this with injected `deps` and drives real 44221 events
 * through the real hook, real policy and real seat plan.
 *
 * **It renders nothing, but it no longer throws anything away.** This
 * component used to take the hook's `outcomes` and drop them on the floor: the
 * host answered hires in the app shell where no session screen is mounted, so
 * a hire could be read, judged, refused and forgotten with nothing on any
 * screen and nothing in any log (ledger draft 97, live 2026-08-30). The hook
 * now publishes every outcome as it lands — see
 * `readCodingSessionHireOutcomes` — and the umbrella's disposition strip
 * renders the counts. Rendering them *here* is not an option: this sits in the
 * shell, above every route, and a shell that painted would paint on every
 * screen in the app.
 */
export function CodingSessionHireRunner(
  props: UseCodingSessionHireInput,
): null {
  useCodingSessionHire(props);
  return null;
}
