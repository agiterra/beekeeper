export type CancelManagedAgentTurnResult = {
  status: "sent" | "no_active_turn";
};

/**
 * Outcome of a live `switch_model` control frame, surfaced asynchronously via
 * the agent's `control_result` observer frame. Busy path: `sent` (cancel +
 * requeue on the new model) or `turn_ending` (oneshot already consumed this
 * turn). Idle path: `switched`, `unsupported_model`, or `no_active_turn`.
 * `ambiguous_target` means a channel-only control cannot choose a thread.
 */
export type SwitchManagedAgentModelStatus =
  | "sent"
  | "turn_ending"
  | "ambiguous_target"
  | "switched"
  | "unsupported_model"
  | "no_active_turn"
  | "failure";

export type ControlResultFrame = {
  type: "cancel_turn" | "switch_model";
  status: string;
  modelId?: string;
  /** Opaque control id, used to ignore a late result for another request. */
  requestId?: string;
  /** Channel carried by the observer envelope, when the result is scoped. */
  channelId?: string | null;
};
