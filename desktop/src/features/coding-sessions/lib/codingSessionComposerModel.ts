/** UI model for the provider-neutral coding-session composer. */
export function getCodingSessionComposerState({
  canSteer,
  isMember,
  isWorking,
  text,
}: {
  /**
   * Whether this execution's runtime advertised native steering. The label is
   * the promise the button makes, so an execution that cannot steer must not
   * offer to: its mid-turn send reaches the provider now and runs at the next
   * turn boundary, which is "Send next", not "Steer".
   *
   * Required, and it used to default to `true`. A call site that forgot it got
   * a Steer button and a `deliver: "steer"` command aimed at a provider whose
   * `threadSteer` is false — a control that is downgraded every single time it
   * is pressed. The safe default for a capability is that it is absent.
   */
  canSteer: boolean;
  isMember: boolean;
  isWorking: boolean;
  text: string;
}) {
  return {
    canSend: isMember && text.trim().length > 0,
    // "Interrupt", never "Stop": the composer also carries **Stop execution**,
    // which is terminal and cannot be undone. Two adjacent buttons both
    // reading Stop is how an operator ends an execution while meaning to end a
    // turn (asked about live, 2026-08-24).
    primaryLabel: isWorking ? "Interrupt" : "Send",
    sendLabel: isWorking ? (canSteer ? "Steer" : "Send next") : "Send",
    showStopAction: isWorking,
    showAuthorityFailure: !isMember,
  };
}

/** Enter steers/sends; Shift+Enter deliberately stays a newline. */
export function shouldSubmitCodingSessionComposerKey(event: {
  key: string;
  shiftKey: boolean;
}): boolean {
  return event.key === "Enter" && !event.shiftKey;
}
