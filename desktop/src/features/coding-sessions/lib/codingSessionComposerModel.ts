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
    primaryLabel: isWorking ? "Stop" : "Send",
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
