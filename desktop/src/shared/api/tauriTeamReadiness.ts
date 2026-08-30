import { invokeTauri } from "@/shared/api/tauri";

export type TeamReadinessState =
  | "ready"
  | "limited"
  | "awaiting_first_session"
  | "blocked"
  | "unknown";

export type TeamReadinessFact = {
  category: string;
  code: string;
  scope: "local" | "wire";
  state: TeamReadinessState;
  summary: string;
  remedy?: string | null;
};

export type TeamReadinessResponse = {
  schemaVersion: number;
  projectRef: string;
  generatedAt: string;
  readyForFirstSession: boolean;
  ready: boolean;
  status: "ready" | "awaiting_first_session" | "blocked" | "unknown";
  hostClass: "cold" | "partial" | "prepared_for_first_session";
  keyStoreSafe: boolean;
  source: {
    appCommit?: string | null;
    appSourceDirty?: boolean | null;
    checkoutPath?: string | null;
    checkoutCommit?: string | null;
    checkoutDirty?: boolean | null;
  };
  ownerPubkey?: string | null;
  team: {
    selectedRoles: string[];
    availableRoles: string[];
    packsDigest?: string | null;
    packsRevision?: string | null;
    packs: Array<{
      role: string;
      personaName: string;
      path: string;
      installedPubkey?: string | null;
    }>;
    identities: Array<{
      role?: string | null;
      personaName?: string | null;
      name: string;
      pubkey: string;
      authTagPresent: boolean;
      profileSync: string;
      sourceRevision?: string | null;
      sourceDigest?: string | null;
      keyState: "live_process" | "in_memory" | "unverified" | "unavailable";
    }>;
  };
  runtimes: Array<{
    instanceRef: string;
    runtime: string;
    label: string;
    adapterPath?: string | null;
    adapterVersion?: string | null;
    auth: "ready" | "needs_auth" | "missing" | "unknown";
    modelProbe: string;
  }>;
  registry: {
    providerTargets: string[];
    coveredTargets: string[];
    uncoveredTargets: string[];
    pendingTargets: string[];
  };
  provider: {
    relayUrl: string;
    provisioned: boolean;
    providerPubkey?: string | null;
    instanceId?: string | null;
    authTagPresent?: boolean | null;
    keyState?:
      | "live_process"
      | "in_memory"
      | "unverified"
      | "unavailable"
      | null;
    process: string;
    childPid?: number | null;
  };
  policy: {
    hiringPolicy: string;
    maxSessions?: number | null;
    turnIdleTimeoutSecs?: number | null;
    turnBudget?: number | null;
  };
  relay: {
    state: TeamReadinessState;
    reachable?: boolean | null;
    source: "local" | "wire";
  };
  catalog: {
    state: TeamReadinessState;
    revision?: number | null;
    targets: string[];
    source: "local" | "wire";
  };
  facts: TeamReadinessFact[];
  blockingCodes: string[];
  unknownCodes: string[];
  awaitingCodes: string[];
  limitedCodes: string[];
};

/** Side-effect-free readiness inventory for one project and intended roster. */
export async function getTeamReadiness(input: {
  projectRef: string;
  selectedRoles: readonly string[];
  hiringPolicyEnabled: boolean;
}): Promise<TeamReadinessResponse> {
  return invokeTauri<TeamReadinessResponse>("team_readiness", {
    projectRef: input.projectRef,
    selectedRoles: [...input.selectedRoles],
    hiringPolicyEnabled: input.hiringPolicyEnabled,
  });
}
