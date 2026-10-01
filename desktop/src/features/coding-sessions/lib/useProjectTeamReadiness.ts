import { useQuery, useQueryClient } from "@tanstack/react-query";
import * as React from "react";

import { readCodingSessionHirePolicy } from "@/features/coding-sessions/lib/codingSessionHirePolicy";
import { useCommunities } from "@/features/communities/useCommunities";
import { restartManagedAgent } from "@/shared/api/tauriManagedAgents";
import {
  ensureCodingSessionProviderRunning,
  provisionCodingSessionProvider,
} from "@/shared/api/tauriSessionProvider";
import {
  installCrewRolePacks,
  scanProjectRolePacks,
  type InstalledCrewRole,
  type ProjectRolePacksScan,
} from "@/shared/api/tauriTeams";
import {
  getTeamReadiness,
  type TeamReadinessResponse,
} from "@/shared/api/tauriTeamReadiness";
import {
  prepareProjectForTeams,
  startSelectedInstalledRoleIdentities,
  type TeamReadinessPrepareStep,
} from "./teamReadinessPrepare";
import { normalizeTeamReadinessRoles } from "./teamReadinessModel";

export const TEAM_READINESS_QUERY_KEY = "team-readiness";

export function teamReadinessScopeKey(input: {
  communityId: string | null;
  relayUrl: string | null;
  channelIds: readonly string[];
  projectRef: string | null;
  checkoutPath: string | null;
  selectedRoles: readonly string[];
  hiringPolicyEnabled: boolean;
}): string {
  return JSON.stringify([
    input.communityId,
    input.relayUrl?.trim().replace(/\/+$/, "").toLowerCase() ?? null,
    [
      ...new Set(
        input.channelIds.map((channel) => channel.trim().toLowerCase()),
      ),
    ].sort(),
    input.projectRef?.trim() ?? null,
    input.checkoutPath?.trim() ?? null,
    normalizeTeamReadinessRoles(input.selectedRoles),
    input.hiringPolicyEnabled,
  ]);
}

export function useProjectTeamReadiness(input: {
  projectRef: string | null;
  checkoutPath: string | null;
  selectedRoles: readonly string[];
  channelIds: readonly string[];
  refreshRuntimeTargets: () => Promise<unknown>;
}) {
  const { activeCommunity } = useCommunities();
  const queryClient = useQueryClient();
  const communityId = activeCommunity?.id ?? null;
  const relayUrl = activeCommunity?.relayUrl ?? null;
  const channelIds = React.useMemo(
    () =>
      [
        ...new Set(
          input.channelIds.map((channel) => channel.trim().toLowerCase()),
        ),
      ].sort(),
    [input.channelIds],
  );
  const channelKey = channelIds.join("\u0000");
  const selectedRoles = React.useMemo(
    () => normalizeTeamReadinessRoles(input.selectedRoles),
    [input.selectedRoles],
  );
  const rolesKey = selectedRoles.join("\u0000");
  const hiringPolicyEnabled = readCodingSessionHirePolicy().enabled;
  const projectRef = input.projectRef?.trim() ?? null;
  const explicitCheckoutPath = input.checkoutPath?.trim() || null;
  const scopeKey = teamReadinessScopeKey({
    communityId,
    relayUrl,
    channelIds,
    projectRef,
    checkoutPath: explicitCheckoutPath,
    selectedRoles,
    hiringPolicyEnabled,
  });
  const currentScopeKey = React.useRef(scopeKey);
  currentScopeKey.current = scopeKey;
  const mountedRef = React.useRef(false);
  const previousScopeKey = React.useRef(scopeKey);
  const query = useQuery({
    queryKey: [
      TEAM_READINESS_QUERY_KEY,
      communityId,
      relayUrl,
      channelKey,
      projectRef,
      explicitCheckoutPath,
      rolesKey,
      hiringPolicyEnabled,
    ],
    queryFn: () =>
      getTeamReadiness({
        projectRef: projectRef as string,
        selectedRoles,
        hiringPolicyEnabled,
        expectedRelayUrl: relayUrl as string,
        channelIds,
      }),
    enabled: projectRef !== null && relayUrl !== null,
    staleTime: 5_000,
  });
  const [freshReadiness, setFreshReadiness] = React.useState<{
    scopeKey: string;
    value: TeamReadinessResponse;
  } | null>(null);
  const [scanState, setScanState] = React.useState<{
    scopeKey: string;
    value: ProjectRolePacksScan;
  } | null>(null);
  const [namesState, setNamesState] = React.useState<{
    scopeKey: string;
    value: Record<string, string>;
  } | null>(null);
  const [prepareSteps, setPrepareSteps] = React.useState<
    TeamReadinessPrepareStep[]
  >([]);
  const [prepareError, setPrepareError] = React.useState<string | null>(null);
  const [prepareWarning, setPrepareWarning] = React.useState<string | null>(
    null,
  );
  const [isScanning, setIsScanning] = React.useState(false);
  const [isPreparing, setIsPreparing] = React.useState(false);
  const [isLaunchPreflighting, setIsLaunchPreflighting] = React.useState(false);
  const scanningRef = React.useRef(false);
  const preparingRef = React.useRef(false);
  const preflightingRef = React.useRef(false);
  const operationGenerationRef = React.useRef(0);
  const readiness =
    freshReadiness?.scopeKey === scopeKey
      ? freshReadiness.value
      : (query.data ?? null);
  const scan = scanState?.scopeKey === scopeKey ? scanState.value : null;
  const names = namesState?.scopeKey === scopeKey ? namesState.value : {};
  const checkoutPath =
    explicitCheckoutPath ?? readiness?.source.checkoutPath ?? null;

  React.useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      operationGenerationRef.current += 1;
    };
  }, []);

  const isCurrentScope = React.useCallback(
    () => mountedRef.current && currentScopeKey.current === scopeKey,
    [scopeKey],
  );
  const requireCurrentScope = React.useCallback(() => {
    if (!isCurrentScope()) {
      throw new Error(
        "The active community or Team Readiness scope changed during Prepare. Start Prepare again in the current community.",
      );
    }
  }, [isCurrentScope]);
  const requireCurrentOperation = React.useCallback(
    (generation: number) => {
      requireCurrentScope();
      if (operationGenerationRef.current !== generation) {
        throw new Error(
          "A newer Team Readiness operation replaced this one. Continue in the current scope.",
        );
      }
    },
    [requireCurrentScope],
  );

  React.useEffect(() => {
    if (previousScopeKey.current === scopeKey) return;
    previousScopeKey.current = scopeKey;
    operationGenerationRef.current += 1;
    scanningRef.current = false;
    preparingRef.current = false;
    preflightingRef.current = false;
    setFreshReadiness(null);
    setScanState(null);
    setNamesState(null);
    setPrepareSteps([]);
    setPrepareError(null);
    setPrepareWarning(null);
    setIsScanning(false);
    setIsPreparing(false);
    setIsLaunchPreflighting(false);
  }, [scopeKey]);

  React.useEffect(
    () => () => {
      queryClient.removeQueries({
        queryKey: [TEAM_READINESS_QUERY_KEY, communityId, relayUrl],
      });
    },
    [communityId, queryClient, relayUrl],
  );

  const beginPrepare = React.useCallback(async () => {
    if (
      scanningRef.current ||
      preparingRef.current ||
      preflightingRef.current
    ) {
      return;
    }
    if (checkoutPath === null) {
      setPrepareError(
        "This project has no local checkout path. Link or clone it before installing its role packs.",
      );
      return;
    }
    requireCurrentScope();
    const generation = operationGenerationRef.current + 1;
    operationGenerationRef.current = generation;
    scanningRef.current = true;
    setIsScanning(true);
    setPrepareError(null);
    setPrepareWarning(null);
    try {
      const result = await scanProjectRolePacks(checkoutPath);
      requireCurrentOperation(generation);
      setScanState({ scopeKey, value: result });
      setNamesState({
        scopeKey,
        value: Object.fromEntries(
          result.packs.map((pack) => [pack.role, pack.defaultName]),
        ),
      });
    } catch (error) {
      if (operationGenerationRef.current === generation && isCurrentScope()) {
        setPrepareError(error instanceof Error ? error.message : String(error));
      }
    } finally {
      if (operationGenerationRef.current === generation) {
        scanningRef.current = false;
        if (isCurrentScope()) setIsScanning(false);
      }
    }
  }, [
    checkoutPath,
    isCurrentScope,
    requireCurrentOperation,
    requireCurrentScope,
    scopeKey,
  ]);

  // A project that names a pack source stages each seat's role from its
  // agents repository at launch; its checkout's `personas/roles` is not where
  // its roles live, so Prepare neither scans nor installs from it (ledger
  // 302(i)) — it prepares the provider and re-reads.
  const rolesStagedAtLaunch = readiness?.team.packSourcePresent === true;

  const runPrepare = React.useCallback(
    async (scanForInstall: typeof scan) => {
      if (
        (!scanForInstall && !rolesStagedAtLaunch) ||
        projectRef === null ||
        relayUrl === null ||
        scanningRef.current ||
        preparingRef.current ||
        preflightingRef.current
      ) {
        return;
      }
      requireCurrentScope();
      const generation = operationGenerationRef.current + 1;
      operationGenerationRef.current = generation;
      preparingRef.current = true;
      setIsPreparing(true);
      setPrepareError(null);
      setPrepareWarning(null);
      let installedRoles: readonly InstalledCrewRole[] = [];
      const result = await prepareProjectForTeams({
        rolesStagedAtLaunch: !scanForInstall,
        dependencies: {
          installRoles: async () => {
            requireCurrentOperation(generation);
            if (!scanForInstall) return;
            const scan = scanForInstall;
            const installed = await installCrewRolePacks(
              scan.directory,
              names,
              relayUrl,
            );
            requireCurrentOperation(generation);
            // A pack refreshed without being asked about is still a pack that
            // can fail, and a silent refresh must not become a hidden failure
            // (finding 15). Every discovered role the install did not produce is
            // named here, alongside the roster roles it dropped.
            const refreshedRoles = new Set(
              installed.installed.map((entry) =>
                entry.role.trim().toLowerCase(),
              ),
            );
            const notRefreshed = scan.packs
              .map((pack) => pack.role.trim().toLowerCase())
              .filter((role) => role.length > 0 && !refreshedRoles.has(role));
            const disclosures = [
              installed.profileSyncError
                ? `Role profiles need another relay sync: ${installed.profileSyncError}`
                : null,
              notRefreshed.length > 0
                ? `These role packs did not refresh: ${[...new Set(notRefreshed)].sort().join(", ")}.`
                : null,
              installed.dropped.length > 0
                ? `These roster roles hold no seat: ${[...installed.dropped].sort().join(", ")}.`
                : null,
            ].filter((line): line is string => line !== null);
            if (disclosures.length > 0)
              setPrepareWarning(disclosures.join(" "));
            installedRoles = installed.installed;
          },
          startRoles: async () => {
            requireCurrentOperation(generation);
            await startSelectedInstalledRoleIdentities({
              installed: installedRoles,
              selectedRoles,
              start: async (pubkey) => {
                requireCurrentOperation(generation);
                const agent = await restartManagedAgent(pubkey, relayUrl);
                requireCurrentOperation(generation);
                return agent;
              },
            });
          },
          provisionProvider: async () => {
            requireCurrentOperation(generation);
            await provisionCodingSessionProvider(relayUrl);
          },
          startProvider: async () => {
            requireCurrentOperation(generation);
            await ensureCodingSessionProviderRunning(relayUrl);
          },
          refreshRuntime: async () => {
            requireCurrentOperation(generation);
            await input.refreshRuntimeTargets();
            requireCurrentOperation(generation);
          },
          rereadReadiness: () => {
            requireCurrentOperation(generation);
            return getTeamReadiness({
              projectRef,
              selectedRoles,
              hiringPolicyEnabled,
              expectedRelayUrl: relayUrl,
              channelIds,
            });
          },
        },
        onSteps: (next) => {
          if (
            operationGenerationRef.current === generation &&
            isCurrentScope()
          ) {
            setPrepareSteps([...next]);
          }
        },
      });
      if (operationGenerationRef.current !== generation || !isCurrentScope()) {
        return;
      }
      if (result.readiness) {
        setFreshReadiness({ scopeKey, value: result.readiness });
      }
      setPrepareError(result.error);
      preparingRef.current = false;
      setIsPreparing(false);
    },
    [
      channelIds,
      hiringPolicyEnabled,
      isCurrentScope,
      names,
      projectRef,
      relayUrl,
      requireCurrentOperation,
      requireCurrentScope,
      rolesStagedAtLaunch,
      scopeKey,
      selectedRoles,
      input.refreshRuntimeTargets,
    ],
  );

  const confirmPrepare = React.useCallback(
    () => runPrepare(scan),
    [runPrepare, scan],
  );

  const startPrepare = React.useCallback(
    () => (rolesStagedAtLaunch ? runPrepare(null) : beginPrepare()),
    [beginPrepare, rolesStagedAtLaunch, runPrepare],
  );

  const readFreshForLaunch = React.useCallback(async () => {
    if (projectRef === null || relayUrl === null) {
      return null;
    }
    if (scanningRef.current || preparingRef.current) {
      throw new Error(
        "Project preparation is still running. Wait for its final readiness re-check.",
      );
    }
    if (preflightingRef.current) {
      throw new Error("A launch readiness check is already running.");
    }
    requireCurrentScope();
    const generation = operationGenerationRef.current + 1;
    operationGenerationRef.current = generation;
    preflightingRef.current = true;
    setIsLaunchPreflighting(true);
    try {
      const value = await getTeamReadiness({
        projectRef,
        selectedRoles,
        hiringPolicyEnabled,
        expectedRelayUrl: relayUrl,
        channelIds,
      });
      if (operationGenerationRef.current !== generation || !isCurrentScope()) {
        throw new Error(
          "The project, checkout, team roles, or hiring policy changed during the readiness check. Try Launch again.",
        );
      }
      setFreshReadiness({ scopeKey, value });
      return value;
    } finally {
      if (operationGenerationRef.current === generation) {
        preflightingRef.current = false;
        if (isCurrentScope()) setIsLaunchPreflighting(false);
      }
    }
  }, [
    channelIds,
    hiringPolicyEnabled,
    isCurrentScope,
    projectRef,
    relayUrl,
    requireCurrentScope,
    scopeKey,
    selectedRoles,
  ]);

  return {
    readiness,
    isLoading: query.isLoading,
    readError:
      query.error instanceof Error
        ? query.error.message
        : query.error
          ? String(query.error)
          : null,
    scan,
    names,
    setName: (role: string, name: string) =>
      setNamesState((current) => ({
        scopeKey,
        value: {
          ...(current?.scopeKey === scopeKey ? current.value : {}),
          [role]: name,
        },
      })),
    beginPrepare: startPrepare,
    confirmPrepare,
    cancelPrepare: () => setScanState(null),
    isScanning,
    isPreparing,
    isLaunchPreflighting,
    readFreshForLaunch,
    prepareSteps,
    prepareError,
    prepareWarning,
  };
}
