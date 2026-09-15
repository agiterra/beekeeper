import * as React from "react";
import {
  useManagedAgentsQuery,
  useUpdateManagedAgentMutation,
} from "@/features/agents/hooks";
import type { ProjectTeamSetupAgents } from "../ui/ProjectTeamSetupAgents";

/**
 * Managed agent names, records (for project association) and in-place rename
 * for installed-role rows. Rename sends only `{pubkey, name}`: same identity,
 * role and association untouched.
 */
export function useProjectTeamSetupAgentDirectory(
  enabled: boolean,
): ProjectTeamSetupAgents {
  const agents = useManagedAgentsQuery({ enabled });
  const { mutateAsync } = useUpdateManagedAgentMutation();
  const names = React.useMemo(
    () =>
      agents.data
        ? new Map(
            agents.data.map((agent) => [
              agent.pubkey.toLowerCase(),
              agent.name,
            ]),
          )
        : null,
    [agents.data],
  );
  const { refetch } = agents;
  const records = agents.data ?? (agents.isError ? "unreadable" : null);
  return React.useMemo(
    () => ({
      names,
      rename: async ({ pubkey, name }) => {
        const result = await mutateAsync({ pubkey, name });
        return { profileSyncError: result.profileSyncError };
      },
      agents: records,
      refreshAgents: () => {
        void refetch();
      },
    }),
    [names, mutateAsync, records, refetch],
  );
}
