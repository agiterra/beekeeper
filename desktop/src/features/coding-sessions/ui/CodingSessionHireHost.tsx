import * as React from "react";
import { useQuery } from "@tanstack/react-query";

import { useChannelsQuery } from "@/features/channels/hooks";
import {
  useAcpRuntimesQuery,
  useManagedAgentsQuery,
} from "@/features/agents/hooks";
import { useProjectsQuery } from "@/features/projects/hooks";
import { isSessionTransportChannel } from "@/shared/api/channelTypes";
import { useIdentityQuery } from "@/shared/api/hooks";
import { getCodingSessionWorkdirState } from "@/shared/api/tauriCodingSessionWorkdirs";
import {
  getCodingSessionProviderRuntimes,
  getCodingSessionProviderStatus,
} from "@/shared/api/tauriSessionProvider";
import { useStableArrayShallow } from "@/shared/hooks/useStableReference";
import {
  useCodingSessionHire,
  type UseCodingSessionHireInput,
} from "../hooks/useCodingSessionHire";
import { readModelRegistry } from "../lib/codingSessionRegistrySource";
import { useGlobalCodingSessionCatalog } from "../useCodingSessionCatalog";
import { groupCodingSessionCatalog } from "../lib/codingSessionUmbrellaModel";
import { useCodingSessionProviderCatalog } from "../useCodingSessionProviderCatalog";
import { resolveCodingSessionHireCatalogSource } from "../lib/codingSessionHireCatalog";
import { codingSessionHireAgentsFromManaged } from "../lib/codingSessionHireCandidates";
import { codingSessionHireRuntimeIdLookup } from "../lib/codingSessionHireAgentRuntime";
import { resolveCodingSessionHireCheckout } from "../lib/codingSessionHireCheckout";

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
  // This computer's ACP catalog, read for one reason: `ManagedAgent.runtime`
  // is the raw per-instance pin and is null for every agent that inherits its
  // harness from its persona, while `agentCommand` is the *effective* harness
  // the native resolver produced. Matching the command against this catalog
  // is exactly what the Agents screen does to draw "Codex"
  // (`AgentInstanceEditDialog.tsx:206-207`), so the hire and the screen a
  // person just looked at cannot disagree (ledger 135(b)). Same query key as
  // the Agents surface, so this costs no extra probe.
  const acpRuntimes = useAcpRuntimesQuery();
  const runtimeIdForCommand = React.useMemo(
    () => codingSessionHireRuntimeIdLookup(acpRuntimes.data ?? []),
    [acpRuntimes.data],
  );
  // Every agent with its project association: a hire is answered only from
  // the umbrella's own project's agents, so the association must not be
  // dropped between the record and the decision.
  const agents = React.useMemo(
    () =>
      codingSessionHireAgentsFromManaged(managedAgents.data ?? [], {
        runtimeIdForCommand,
      }),
    [managedAgents.data, runtimeIdForCommand],
  );
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
  const providerCatalog = useCodingSessionProviderCatalog(channelIds);
  const catalogForHire = React.useCallback(
    (input: { channelId: string; projectRef: string | null }) => {
      const signerPubkey = providerStatus.data?.providerPubkey;
      if (!signerPubkey) return null;
      return resolveCodingSessionHireCatalogSource({
        entries: providerCatalog.entries,
        channelId: input.channelId,
        signerPubkey,
        projectRef: input.projectRef,
      });
    },
    [providerCatalog.entries, providerStatus.data?.providerPubkey],
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

  // Project names, for the refusal that tells a person which project to set a
  // repository folder for. A coordinate alone ("30621:abc…:tank-loop") is not
  // a control anybody can find.
  const projects = useProjectsQuery();
  const projectLabels = React.useMemo(() => {
    const labels = new Map<string, string>();
    for (const project of projects.data ?? []) {
      const address = project.projectAddress?.trim();
      const name = project.name?.trim();
      if (address && name) labels.set(address, name);
    }
    return labels;
  }, [projects.data]);

  const workdirState = workdirs.data ?? null;
  const checkoutForHire = React.useCallback(
    (input: { channelId: string; projectRef: string | null }) =>
      // `mru` is deliberately not passed: the rule has no use for it, and it
      // is what cut two Tank Loop seats from the Beekeeper repository on
      // 2026-09-16 (ledger 135(a)).
      resolveCodingSessionHireCheckout({
        projectRef: input.projectRef,
        projectLabel: projectLabels.get(input.projectRef?.trim() ?? "") ?? null,
        channelId: input.channelId,
        byProject: workdirState?.byProject ?? {},
        byChannel: workdirState?.byChannel ?? {},
      }),
    [projectLabels, workdirState],
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
      agents={agents}
      catalogForHire={catalogForHire}
      channelIds={channelIds}
      checkoutForHire={checkoutForHire}
      operatorPubkey={identityQuery.data?.pubkey ?? null}
      registryForProject={readModelRegistry}
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
