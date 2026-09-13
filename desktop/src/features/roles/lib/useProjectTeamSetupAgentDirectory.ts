import * as React from "react";
import {
  useManagedAgentsQuery,
  useUpdateManagedAgentMutation,
} from "@/features/agents/hooks";
import type { ProjectTeamSetupAgents } from "../ui/ProjectTeamSetupAgents";

/**
 * Managed agent names and in-place rename for installed-role rows. Rename
 * sends only `{pubkey, name}`: same identity, role untouched.
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
  return React.useMemo(
    () => ({
      names,
      rename: async ({ pubkey, name }) => {
        const result = await mutateAsync({ pubkey, name });
        return { profileSyncError: result.profileSyncError };
      },
    }),
    [names, mutateAsync],
  );
}
