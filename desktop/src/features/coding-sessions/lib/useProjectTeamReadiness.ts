import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { readCodingSessionHirePolicy } from "@/features/coding-sessions/lib/codingSessionHirePolicy";
import {
  ensureCodingSessionProviderRunning,
  provisionCodingSessionProvider,
} from "@/shared/api/tauriSessionProvider";
import {
  installCrewRolePacks,
  scanProjectRolePacks,
  type ProjectRolePacksScan,
} from "@/shared/api/tauriTeams";
import {
  getTeamReadiness,
  type TeamReadinessResponse,
} from "@/shared/api/tauriTeamReadiness";
import {
  prepareProjectForTeams,
  type TeamReadinessPrepareStep,
} from "./teamReadinessPrepare";
import { normalizeTeamReadinessRoles } from "./teamReadinessModel";

export const TEAM_READINESS_QUERY_KEY = "team-readiness";

export function teamReadinessScopeKey(input: {
  projectRef: string | null;
  checkoutPath: string | null;
  selectedRoles: readonly string[];
  hiringPolicyEnabled: boolean;
}): string {
  return JSON.stringify([
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
}) {
  const selectedRoles = React.useMemo(
    () => normalizeTeamReadinessRoles(input.selectedRoles),
    [input.selectedRoles],
  );
  const rolesKey = selectedRoles.join("\u0000");
  const hiringPolicyEnabled = readCodingSessionHirePolicy().enabled;
  const projectRef = input.projectRef?.trim() ?? null;
  const explicitCheckoutPath = input.checkoutPath?.trim() || null;
  const scopeKey = teamReadinessScopeKey({
    projectRef,
    checkoutPath: explicitCheckoutPath,
    selectedRoles,
    hiringPolicyEnabled,
  });
  const currentScopeKey = React.useRef(scopeKey);
  currentScopeKey.current = scopeKey;
  const previousScopeKey = React.useRef(scopeKey);
  const query = useQuery({
    queryKey: [
      TEAM_READINESS_QUERY_KEY,
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
      }),
    enabled: projectRef !== null,
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
  const [isScanning, setIsScanning] = React.useState(false);
  const [isPreparing, setIsPreparing] = React.useState(false);
  const [isLaunchPreflighting, setIsLaunchPreflighting] = React.useState(false);
  const scanningRef = React.useRef(false);
  const preparingRef = React.useRef(false);
  const preflightingRef = React.useRef(false);
  const readiness =
    freshReadiness?.scopeKey === scopeKey
      ? freshReadiness.value
      : (query.data ?? null);
  const scan = scanState?.scopeKey === scopeKey ? scanState.value : null;
  const names = namesState?.scopeKey === scopeKey ? namesState.value : {};
  const checkoutPath =
    explicitCheckoutPath ?? readiness?.source.checkoutPath ?? null;

  React.useEffect(() => {
    if (previousScopeKey.current === scopeKey) return;
    previousScopeKey.current = scopeKey;
    setFreshReadiness(null);
    setScanState(null);
    setNamesState(null);
    setPrepareSteps([]);
    setPrepareError(null);
  }, [scopeKey]);

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
    scanningRef.current = true;
    setIsScanning(true);
    setPrepareError(null);
    try {
      const result = await scanProjectRolePacks(checkoutPath);
      setScanState({ scopeKey, value: result });
      setNamesState({
        scopeKey,
        value: Object.fromEntries(
          result.packs.map((pack) => [pack.role, pack.defaultName]),
        ),
      });
    } catch (error) {
      setPrepareError(error instanceof Error ? error.message : String(error));
    } finally {
      scanningRef.current = false;
      setIsScanning(false);
    }
  }, [checkoutPath, scopeKey]);

  const confirmPrepare = React.useCallback(async () => {
    if (
      !scan ||
      projectRef === null ||
      scanningRef.current ||
      preparingRef.current ||
      preflightingRef.current
    ) {
      return;
    }
    preparingRef.current = true;
    setIsPreparing(true);
    setPrepareError(null);
    const result = await prepareProjectForTeams({
      dependencies: {
        installRoles: async () => {
          await installCrewRolePacks(scan.directory, names);
        },
        provisionProvider: async () => {
          await provisionCodingSessionProvider();
        },
        startProvider: async () => {
          await ensureCodingSessionProviderRunning();
        },
        rereadReadiness: () =>
          getTeamReadiness({
            projectRef,
            selectedRoles,
            hiringPolicyEnabled,
          }),
      },
      onSteps: (next) => setPrepareSteps([...next]),
    });
    if (result.readiness) {
      if (currentScopeKey.current === scopeKey) {
        setFreshReadiness({ scopeKey, value: result.readiness });
      }
    }
    setPrepareError(result.error);
    preparingRef.current = false;
    setIsPreparing(false);
  }, [hiringPolicyEnabled, names, projectRef, scan, scopeKey, selectedRoles]);

  const readFreshForLaunch = React.useCallback(async () => {
    if (projectRef === null) return null;
    if (scanningRef.current || preparingRef.current) {
      throw new Error(
        "Project preparation is still running. Wait for its final readiness re-check.",
      );
    }
    if (preflightingRef.current) {
      throw new Error("A launch readiness check is already running.");
    }
    preflightingRef.current = true;
    setIsLaunchPreflighting(true);
    try {
      const value = await getTeamReadiness({
        projectRef,
        selectedRoles,
        hiringPolicyEnabled,
      });
      if (currentScopeKey.current !== scopeKey) {
        throw new Error(
          "The project, checkout, team roles, or hiring policy changed during the readiness check. Try Launch again.",
        );
      }
      setFreshReadiness({ scopeKey, value });
      return value;
    } finally {
      preflightingRef.current = false;
      setIsLaunchPreflighting(false);
    }
  }, [hiringPolicyEnabled, projectRef, scopeKey, selectedRoles]);

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
    beginPrepare,
    confirmPrepare,
    cancelPrepare: () => setScanState(null),
    isScanning,
    isPreparing,
    isLaunchPreflighting,
    readFreshForLaunch,
    prepareSteps,
    prepareError,
  };
}
