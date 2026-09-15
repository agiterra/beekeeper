import * as React from "react";
import type { ManagedAgent } from "@/shared/api/types";
import { truncatePubkey } from "@/shared/lib/pubkey";

/** Rename one managed agent in place; only `{pubkey, name}` is ever sent. */
export type RenameProjectTeamSetupAgent = (input: {
  pubkey: string;
  name: string;
}) => Promise<{ profileSyncError: string | null }>;

/**
 * The managed agents this computer knows, for naming installed identities.
 * `names === null` means the list was not read (not that the agents have no
 * names); `rename === null` hides rename rather than offering a dead control.
 */
export type ProjectTeamSetupAgents = {
  names: ReadonlyMap<string, string> | null;
  rename: RenameProjectTeamSetupAgent | null;
  /**
   * The managed agent records, read for project association. Absent or
   * `null` while unread; `"unreadable"` when the read failed. Setup never
   * reads as ready on anything but a read list.
   */
  agents?: readonly ManagedAgent[] | null | "unreadable";
  /** Re-read the agent records (after an install or a failed read). */
  refreshAgents?: (() => void) | null;
  /** Open the project's Agents tab; `null` hides the link. */
  openAgentsTab?: (() => void) | null;
  /** Open the started lead's session; `null` hides the action. */
  openLeadSession?:
    | ((lead: { channelId: string; sessionRef: string }) => void)
    | null;
};

const NO_AGENTS: ProjectTeamSetupAgents = { names: null, rename: null };

export const ProjectTeamSetupAgentsContext =
  React.createContext<ProjectTeamSetupAgents>(NO_AGENTS);

export function useProjectTeamSetupAgents(): ProjectTeamSetupAgents {
  return React.useContext(ProjectTeamSetupAgentsContext);
}

/** First and last characters of a pubkey, enough to tell identities apart. */
export function shortPubkey(pubkey: string): string {
  return truncatePubkey(pubkey);
}

/** An agent's current name, looked up case-insensitively by pubkey. */
export function agentName(
  names: ReadonlyMap<string, string> | null,
  pubkey: string,
): string | null {
  return names?.get(pubkey.toLowerCase()) ?? null;
}
