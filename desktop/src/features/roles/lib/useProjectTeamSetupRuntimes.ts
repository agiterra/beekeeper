import * as React from "react";
import {
  getCodingSessionProviderModels,
  getCodingSessionProviderRuntimes,
  getCodingSessionProviderStatus,
  type CodingSessionProviderModels,
  type CodingSessionProviderRuntime,
  type CodingSessionProviderStatus,
} from "@/shared/api/tauriSessionProvider";
import { projectTeamSetupError } from "./projectTeamSetup";

export type ProjectTeamSetupRuntimes = {
  status: CodingSessionProviderStatus;
  runtimes: CodingSessionProviderRuntime[];
  models: Map<string, CodingSessionProviderModels>;
  errors: Map<string, string>;
};

/** Probe local availability without minting identities or assuming a runtime. */
export async function loadProjectTeamSetupRuntimes(
  deps = {
    status: getCodingSessionProviderStatus,
    runtimes: getCodingSessionProviderRuntimes,
    models: getCodingSessionProviderModels,
  },
): Promise<ProjectTeamSetupRuntimes> {
  const [status, runtimes] = await Promise.all([
    deps.status(),
    deps.runtimes(),
  ]);
  const models = new Map<string, CodingSessionProviderModels>();
  const errors = new Map<string, string>();
  await Promise.all(
    runtimes
      .filter(
        (runtime) =>
          runtime.authState === "ready" && runtime.capabilities.threadTurnStart,
      )
      .map(async (runtime) => {
        try {
          const catalog = await deps.models(runtime.instanceRef);
          if (
            catalog.instanceRef !== runtime.instanceRef ||
            catalog.allowedModels.length === 0
          )
            throw new Error(
              "This runtime did not return a usable model catalog.",
            );
          models.set(runtime.instanceRef, catalog);
        } catch (error) {
          errors.set(runtime.instanceRef, projectTeamSetupError(error));
        }
      }),
  );
  return { status, runtimes, models, errors };
}

/** Read-only discovery; refresh replaces stale catalogs, including failures. */
export function useProjectTeamSetupRuntimes(scopeKey: string) {
  const [data, setData] = React.useState<ProjectTeamSetupRuntimes | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [loading, setLoading] = React.useState(true);
  const generation = React.useRef(0);
  const refresh = React.useCallback(async () => {
    const request = ++generation.current;
    setLoading(true);
    setError(null);
    try {
      const next = await loadProjectTeamSetupRuntimes();
      if (request === generation.current) setData(next);
      return next;
    } catch (failure) {
      if (request === generation.current) {
        setData(null);
        setError(projectTeamSetupError(failure));
      }
      throw failure;
    } finally {
      if (request === generation.current) setLoading(false);
    }
  }, []);
  React.useEffect(() => {
    void scopeKey;
    setData(null);
    void refresh().catch(() => {});
    return () => {
      generation.current += 1;
    };
  }, [scopeKey, refresh]);
  return { data, error, loading, refresh };
}
