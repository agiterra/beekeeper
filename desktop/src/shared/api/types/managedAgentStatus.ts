/** Backend-reported lifecycle status; host ownership does not imply availability. */
export type ManagedAgentStatus =
  | "running"
  | "stopped"
  | "deployed"
  | "not_deployed"
  | "host_managed"
  | "recreation_required";
