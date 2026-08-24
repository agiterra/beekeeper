/** UI model for the provider-neutral coding-session composer. */
export function getCodingSessionComposerState({
  isMember,
  isWorking,
  text,
}: {
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
    sendLabel: isWorking ? "Steer" : "Send",
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
